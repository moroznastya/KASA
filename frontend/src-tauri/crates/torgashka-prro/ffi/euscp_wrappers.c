/*
 * Обгортки для багнутого SDK EUSignCP (euscp.so / cspb.so).
 *
 * Баг №1 (euscp.so): EUReadPrivateKeyBinary використовує callee-saved регістр
 * %rbx БЕЗ ініціалізації (очікує %rbx == 0 від калера). Python-ctypes
 * випадково лишає %rbx == 0; C/Rust-калери лишають довільне значення
 * (часто адресу функції) → SDK виконує `movl $0,(%rbx)` → запис у .text →
 * SIGSEGV. Діагноз: дизасемблер `movl $0x0,(%rbx)` при %rbx == адреса
 * функції; у Python-процесі на вході %rbx == 0 (gdb-виміряно).
 *
 * Баг №2 (cspb.so): внутрішній код копіює блок через `movdqa` (aligned,
 * вимагає 16-байт вирівнювання) на адресу `rsp-0x38`, що валідна лише
 * якщо на вході у функцію %rsp ≡ 8 (mod 16). Будь-який зсув стеку на 8
 * байт (наприклад, `push %rbx` у обгортці) ламає вирівнювання →
 * #GP → SIGSEGV. Відтворено у RELEASE-збірках (debug-збірки випадково
 * лишають правильне вирівнювання): general protection fault у cspb.so
 * на зміщенні 0x7a925 (ECDHDSTUSelfTest, `mov %r8d,0x4(%rdx)`) — процес
 * вмирає БЕЗ panic-повідомлення (жорсткий #GP від ядра).
 *
 * Рішення: naked-обгортка, яка ПЕРЕД викликом SDK:
 *   - переносить аргументи SysV (rdi/rsi/rdx/rcx) у потрібні позиції;
 *   - встановлює %rbx = 0 і %rcx = 0 (SDK-баги; Python-ctypes лишає обидва
 *     нульовими);
 *   - примусово вирівнює %rsp: на вході обгортки rsp ≡ 8 (mod 16) (SysV),
 *     `sub $8` → rsp ≡ 0 перед `call`, після push return-адреси SDK бачить
 *     rsp ≡ 8 (mod 16) — те, що вимагає cspb.so.
 * Оригінальний %rbx зберігається у %r11 (caller-saved, НЕ на стек — щоб
 * не змінити вирівнювання %rsp) і відновлюється після повернення.
 */
#include <stdint.h>

typedef int (*FnReadPrivateKeyBinary)(const char *, int, const char *);

__attribute__((naked, noinline))
int eu_read_private_key_binary_rbx0(FnReadPrivateKeyBinary fn,
                                    const char *key, int key_len,
                                    const char *password) {
    __asm__ volatile(
        "mov %rdi, %rax\n\t"   /* fn → rax (для call)                          */
        "mov %rsi, %rdi\n\t"   /* key → rdi (1-й арг SDK)                      */
        "mov %rdx, %rsi\n\t"   /* key_len → rsi (2-й арг SDK)                  */
        "mov %rcx, %rdx\n\t"   /* password → rdx (3-й арг SDK)                 */
        "xor %ecx, %ecx\n\t"   /* SDK-баг: очікує rcx==0 (інакше rc=24)        */
        "mov %rbx, %r11\n\t"   /* зберегти callee-saved rbx (НЕ на стек)       */
        "xor %ebx, %ebx\n\t"   /* SDK-баг: очікує rbx==0 від калера            */
        "sub $8, %rsp\n\t"     /* вирівнювання: rsp≡8 → 0 перед call → SDK≡8   */
        "call *%rax\n\t"       /* викликати EUReadPrivateKeyBinary             */
        "add $8, %rsp\n\t"     /* повернути стек                               */
        "mov %r11, %rbx\n\t"   /* відновити rbx                                */
        "ret\n\t"
    );
}
