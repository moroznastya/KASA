"""Seed: оновлення шаблону receipt_58mm через psycopg2"""
import psycopg2

NEW_CONTENT = """<html>
<body style="font-family: 'Arial', sans-serif; font-size: 12px; width: 48mm; margin: 0; padding: 0; color: #000; line-height: 1.2;">
    <div style="padding: 2px 4px;">
        
        <!-- Шапка: Інформація про магазин -->
        <div style="text-align: center; margin-bottom: 8px;">
            <div style="font-size: 16px; font-weight: bold; text-transform: uppercase; margin-bottom: 2px;">{{shop_name}}</div>
            <div style="font-size: 10px;">{{shop_address}}</div>
           
        </div>

        <div style="border-top: 1px dashed #000; margin: 6px 0;"></div>

        <!-- Інформація про чек та касира -->
        <div style="text-align: center; font-size: 13px; font-weight: bold; margin: 4px 0;">
            ЧЕК № {{receipt_number}}
        </div>
        <div style="font-size: 10px; margin-bottom: 4px;">
            <table style="width: 100%; border-collapse: collapse; font-size: 10px;">
                <tr>
                    <td style="text-align: left;">{{date}}</td>
                    <td style="text-align: right;">{{time}}</td>
                </tr>
                <tr>
                    <td style="text-align: left;" colspan="2">Касир: {{cashier}}</td>
                </tr>
            </table>
        </div>

        <div style="border-top: 1px dashed #000; margin: 6px 0;"></div>

        <!-- Список товарів (без font-family/font-size — inline стилі) -->
        <div style="width: 100%; margin: 4px 0;">
            {{items}}
        </div>

        <div style="border-top: 1px dashed #000; margin: 6px 0;"></div>

        <!-- Підсумок -->
        <table style="width: 100%; border-collapse: collapse; margin-top: 4px;">
            <tr>
                <td style="font-size: 14px; font-weight: bold; text-align: left;">ДО СПЛАТИ</td>
                <td style="font-size: 14px; font-weight: bold; text-align: right;">{{total}} грн</td>
            </tr>
        </table>

        <!-- Деталі оплати -->
        <table style="width: 100%; border-collapse: collapse; font-size: 11px; margin-top: 4px;">
            <tr>
                <td style="text-align: left;">{{payment_method}}</td>
                <td style="text-align: right;">{{paid}} грн</td>
            </tr>
            <tr>
                <td style="text-align: left;">Решта</td>
                <td style="text-align: right;">{{change}} грн</td>
            </tr>
        </table>

        <div style="border-top: 1px dashed #000; margin: 8px 0 6px 0;"></div>

        <!-- Підвал -->
        <div style="text-align: center; font-size: 12px; font-weight: bold; margin-top: 4px;">
            Дякуємо за покупку!
        </div>
        <div style="text-align: center; font-size: 10px; margin-top: 4px; font-style: italic;">
            Ми цінуємо ваш вибір і сподіваємося побачити вас знову.
        </div>

    </div>
</body>
</html>"""

conn = psycopg2.connect(
    host='localhost',
    port=5432,
    dbname='pos_system',
    user='postgres',
    password='VgxWd7MBJ10X'
)
cur = conn.cursor()
cur.execute(
    "UPDATE print_templates SET content = %s WHERE id = 'a0000000-0000-0000-0000-000000000001'",
    (NEW_CONTENT,)
)
conn.commit()
print(f"✅ Оновлено рядків: {cur.rowcount}")
cur.close()
conn.close()
