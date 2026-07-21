"""
Третій етап категоризації товарів.
"""
import asyncio
from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession, async_sessionmaker
from app.config import settings

async def categorize():
    engine = create_async_engine(settings.DATABASE_URL, echo=False)
    session_factory = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)
    async with session_factory() as session:
        result = await session.execute(text("SELECT id, name FROM categories"))
        cat_map = {r.name: r.id for r in result.all()}

        result = await session.execute(text("""
            SELECT p.id, p.title, p.barcode
            FROM products p
            JOIN categories c ON c.id = p.category_id
            WHERE c.name = 'Без категорії'
            ORDER BY p.title
        """))
        products = result.all()

        updates = []
        uncertain = []

        for p in products:
            title = p.title.lower()
            cat_id = None
            reason = ""

            # === АЛКОГОЛЬ (додаткові) ===
            if any(w in title for w in ["десант", "зіберт", "zibert", "карлсберг", "carlsberg", "туборг", "tuborg", "хайк", "hike", "каньйон", "canyon"]):
                cat_id = cat_map.get("Пиво")
                reason = "Пиво"
            elif any(w in title for w in ["виннап", "вин.нап", "фраголіно", "fragolino", "вермонте"]):
                cat_id = cat_map.get("Вина")
                reason = "Вина"
            elif any(w in title for w in ["фраг", "фрат", "frattina"]):
                cat_id = cat_map.get("Вина")
                reason = "Вина"

            # === НАПОЇ ===
            elif any(w in title for w in ["каньйон", "canyon", "дюшес", "duchesse"]):
                cat_id = cat_map.get("Солодка вода")
                reason = "Солодка вода"

            # === СОЛОДОЩІ (додаткові) ===
            elif any(w in title for w in ["зефір", "zephyr", "marshmallow"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Зефір"
            elif any(w in title for w in ["халв", "halva"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Халва"
            elif any(w in title for w in ["дірол", "dirol", "жув.гумк", "жувальн гумк", "gum", "орбіт", "orbit", "стимул", "stimorol"]):
                cat_id = cat_map.get("Жуйки")
                reason = "Жуйки"
            elif any(w in title for w in ["чупачупс", "chupachups", "chupa chups", "льодяник", "lollipop"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Льодяники"
            elif any(w in title for w in ["какао", "cocoa"]):
                cat_id = cat_map.get("Кава")
                reason = "Какао"
            elif any(w in title for w in ["капучіно", "cappuccino", "капучино", "лате", "latte"]):
                cat_id = cat_map.get("Кава")
                reason = "Кава"
            elif any(w in title for w in ["вафельні корж", "вафельні лист", "вафельні трубочк", "вафельн корж"]):
                cat_id = cat_map.get("Печиво, вафлі, бісквіт")
                reason = "Вафельні вироби"
            elif any(w in title for w in ["сухар", "сухарі", "тост", "toast", "грісіні", "grissini", "грізіні", "соломк", "смачк", "croco"]):
                cat_id = cat_map.get("Сухарики, снеки")
                reason = "Сухарики, снеки"
            elif any(w in title for w in ["кокосова стружк", "кокос стружк"]):
                cat_id = cat_map.get("Печиво, вафлі, бісквіт")
                reason = "Кокосова стружка"
            elif any(w in title for w in ["цукат", "candied"]):
                cat_id = cat_map.get("Сухофрукти")
                reason = "Цукати"
            elif any(w in title for w in ["коктейль молочн", "milkshake", "молочний коктейль"]):
                cat_id = cat_map.get("Молоко, йогурти, кефір")
                reason = "Молочний коктейль"
            elif any(w in title for w in ["трубочка згущонк", "трубочка згущен"]):
                cat_id = cat_map.get("Згущене молоко")
                reason = "Трубочка зі згущеним молоком"
            elif any(w in title for w in ["цук шарм", "цукерки шарм", "biguin", "цукери"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Цукерки"
            elif any(w in title for w in ["цукрова пудр", "цукор пудр"]):
                cat_id = cat_map.get("Сіль, цукор")
                reason = "Цукрова пудра"
            elif any(w in title for w in ["канелон", "cannelloni"]):
                cat_id = cat_map.get("Макаронні вироби")
                reason = "Канелони"
            elif any(w in title for w in ["кисель", "kissel"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Кисель"
            elif any(w in title for w in ["кокосова стружк"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Кокосова стружка"
            elif any(w in title for w in ["сухі сніданк", "сухий сніданок", "золоте зерно"]):
                cat_id = cat_map.get("Крупи")
                reason = "Сухі сніданки"
            elif any(w in title for w in ["фасол", "fasol"]):
                cat_id = cat_map.get("Крупи")
                reason = "Фасоля"

            # === РИБА (додаткові) ===
            elif any(w in title for w in ["кільк", "тюльк", "телапія", "tilapia", "філе мінтая", "філе осел", "тунець", "tuna", "tunczyk"]):
                cat_id = cat_map.get("Заморожена")
                reason = "Заморожена риба"
            elif any(w in title for w in ["журавлин"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Журавлина"

            # === М'ЯСО (додаткові) ===
            elif any(w in title for w in ["смалець", "lard"]):
                cat_id = cat_map.get("Інша м'ясна продукція")
                reason = "Смалець"
            elif any(w in title for w in ["ковб домашн", "ковбаса домашн"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Ковбаса домашня"
            elif any(w in title for w in ["закусна бутербродн", "бутербродиво"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Закусна бутербродна"
            elif any(w in title for w in ["кишк", "kiszka"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Кишка"

            # === ПОБУТОВА ХІМІЯ (додаткові) ===
            elif any(w in title for w in ["дихлор", "дихлофос", "засіб від комах", "от мышей", "от крыс", "bros", "липкая лента от мух"]):
                cat_id = cat_map.get("Засоби захисту від комарів, сонця")
                reason = "Засоби від комах"
            elif any(w in title for w in ["засіб для чищення", "для чищення", "чистяще", "чистячий", "cleaner", "well done", "astonish", "рідина для миття", "миття ванн", "миття кухн", "миття туалет", "туалетний утенок", "туалетный утенок", "средство для унитаза", "засіб їжак", "засіб для душу", "для чищення душу", "камінь і ржавчина", "титан", "w5", "запаска", "zapaska", "bispol", "chante clair", "chanteclar"]):
                cat_id = cat_map.get("Хімія")
                reason = "Хімія"
            elif any(w in title for w in ["капсул", "savex", "silla", "капсули для прання"]):
                cat_id = cat_map.get("Пральні порошки")
                reason = "Капсули для прання"
            elif any(w in title for w in ["стрейч плівк", "strech", "плівка"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Стрейч плівка"
            elif any(w in title for w in ["стиральна гумк", "гумка"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Стиральна гумка"

            # === ЗАСОБИ ГІГІЄНИ (додаткові) ===
            elif any(w in title for w in ["зуб. щетк", "зубна щітк", "зуб щітк"]):
                cat_id = cat_map.get("Зубні пасти")
                reason = "Зубні щітки"
            elif any(w in title for w in ["шамп", "shampoo", "pantene", "шампун"]):
                cat_id = cat_map.get("Косметика та дезодоранти")
                reason = "Шампунь"
            elif any(w in title for w in ["тонуюча маск", "маска для волосся", "фарба волос", "рябина"]):
                cat_id = cat_map.get("Фарба для волосся")
                reason = "Фарба/маска для волосся"
            elif any(w in title for w in ["зажим для волосся", "заколк", "резинк для волосся", "обруч", "гребін", "щітка для волосся"]):
                cat_id = cat_map.get("Аксесуари для гігієни")
                reason = "Аксесуари для волосся"
            elif any(w in title for w in ["колгот", "tights", "панчох"]):
                cat_id = cat_map.get("Шкарпетки")
                reason = "Колготи"

            # === ДЛЯ ДОМУ (додаткові) ===
            elif any(w in title for w in ["скрепк", "скріпк", "paperclip"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Канцелярія"
            elif any(w in title for w in ["карандаш", "олівець", "pencil"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Канцелярія"
            elif any(w in title for w in ["карти", "гральні карти", "playing cards"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Канцелярія"
            elif any(w in title for w in ["фонарик", "фонарь", "ліхтар", "flashlight"]):
                cat_id = cat_map.get("Електрика")
                reason = "Ліхтарик"
            elif any(w in title for w in ["термос", "thermos"]):
                cat_id = cat_map.get("Посуд")
                reason = "Термос"
            elif any(w in title for w in ["форма на паск", "форма для випічки"]):
                cat_id = cat_map.get("Посуд")
                reason = "Форма для випічки"
            elif any(w in title for w in ["дротик", "dart"]):
                cat_id = cat_map.get("Все для свята")
                reason = "Дротик"
            elif any(w in title for w in ["бульбашк", "bubble"]):
                cat_id = cat_map.get("Все для свята")
                reason = "Бульбашки"
            elif any(w in title for w in ["дратв", "нитк"]):
                cat_id = cat_map.get("Рукоділля")
                reason = "Дратва/нитки"
            elif any(w in title for w in ["спиртометр"]):
                cat_id = cat_map.get("Для дому")
                reason = "Спиртометр"

            # === ТЮТЮН (додаткові) ===
            elif any(w in title for w in ["зажигалк", "fox", "запальничк"]):
                cat_id = cat_map.get("Запальнички")
                reason = "Запальнички"
            elif any(w in title for w in ["тютюн", "tobacco", "virginia", "вірджинія"]):
                cat_id = cat_map.get("Інші сигарети")
                reason = "Тютюн"

            # === НАСІННЯ ===
            elif any(w in title for w in ["насінн", "айстр", "іберіс", "циннія", "цинія", "чорнобривц", "томат", "помідор насін", "квіт", "flover", "seed"]):
                cat_id = cat_map.get("Усе насіння")
                reason = "Насіння квітів/рослин"

            # === ЗАМОРОЖЕНА ПРОДУКЦІЯ ===
            elif any(w in title for w in ["пломбір", "морозив", "хрещатик"]):
                cat_id = cat_map.get("Морозиво і десерти")
                reason = "Морозиво"

            # === ФРУКТИ, ОВОЧІ ===
            elif any(w in title for w in ["чері", "cherry", "черешн", "вишн"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Черешня/вишня"
            elif any(w in title for w in ["гранат", "pomegranate"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Гранат"
            elif any(w in title for w in ["груш", "pear"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Груша"

            # === БАКАЛІЯ ===
            elif any(w in title for w in ["галаретк", "galaretka", "желе"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Галаретка/желе"
            elif any(w in title for w in ["гірчичний порошок", "гірчиц порошок"]):
                cat_id = cat_map.get("Спеції")
                reason = "Гірчичний порошок"
            elif any(w in title for w in ["суміш перців", "перець суміш"]):
                cat_id = cat_map.get("Спеції")
                reason = "Суміш перців"
            elif any(w in title for w in ["борошно", "мука"]):
                cat_id = cat_map.get("Борошно")
                reason = "Борошно"

            # === ПАКЕТИ ===
            elif any(w in title for w in ["фасовк", "еко-пак", "eco-pack", "пакет"]):
                cat_id = cat_map.get("Пакети")
                reason = "Пакети"

            # === ТОВАРИ ДЛЯ ДІТЕЙ ===
            elif any(w in title for w in ["яйце сюрприз", "яйце-сюрприз", "іграшк", "toy", "roblox", "funny egg"]):
                cat_id = cat_map.get("Товари для дітей")
                reason = "Іграшки"

            if cat_id:
                updates.append((p.id, cat_id, reason))
            else:
                uncertain.append(p)

        print(f"Визначено: {len(updates)} товарів")
        print(f"Залишилось невизначеними: {len(uncertain)}")

        for prod_id, cat_id, reason in updates:
            await session.execute(
                text("UPDATE products SET category_id = :cat_id WHERE id = :prod_id"),
                {"cat_id": cat_id, "prod_id": prod_id}
            )

        await session.commit()
        print(f"✅ Оновлено {len(updates)} товарів")

        print("\n=== НЕВИЗНАЧЕНІ ТОВАРИ ===")
        for p in uncertain:
            print(f"  {p.title}  (ШК: {p.barcode or '-'})")

        with open("/tmp/uncertain_products_3.txt", "w", encoding="utf-8") as f:
            for p in uncertain:
                f.write(f"{p.title}|{p.barcode or ''}|{p.id}\n")
        print(f"\nНевизначені збережено у /tmp/uncertain_products_3.txt")

    await engine.dispose()

asyncio.run(categorize())
