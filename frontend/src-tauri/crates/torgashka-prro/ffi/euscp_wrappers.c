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
 * #GP → SIGSEGV.
 *
 * Рішення: обгортка встановлює %rbx = 0 і %rcx = 0 ПЕРЕД викликом SDK
 * (Python-ctypes лишає обидва нульовими; C/Rust — довільні значення, що
 * дають rc=24 «невірний пароль» або SIGSEGV). Оригінальний %rbx
 * зберігається у %r11 (caller-saved, НЕ на стек — щоб не змінити
 * вирівнювання %rsp). Після повернення %rbx відновлюється.
 */
#include <stdint.h>

typedef int (*FnReadPrivateKeyBinary)(const char *, int, const char *);

__attribute__((noinline))
int eu_read_private_key_binary_rbx0(FnReadPrivateKeyBinary fn,
                                    const char *key, int key_len,
                                    const char *password) {
    int rc;
    __asm__ volatile(
        "mov %%rbx, %%r11\n\t"   /* зберегти callee-saved rbx (НЕ на стек) */
        "xorl %%ebx, %%ebx\n\t"  /* SDK-баг: очікує rbx==0 від калера       */
        "xorl %%ecx, %%ecx\n\t"  /* SDK-баг: очікує rcx==0 (інакше rc=24)  */
        "call *%%rax\n\t"        /* викликати EUReadPrivateKeyBinary        */
        "mov %%r11, %%rbx\n\t"   /* відновити rbx                           */
        : "=a"(rc)
        : "0"(fn), "D"(key), "S"(key_len), "d"(password)
        : "r11", "rcx", "r8", "r9", "r10", "memory");
    return rc;
}
