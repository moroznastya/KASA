"""
Четвертий етап категоризації товарів.
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

            # Товари з назвою-штрихкодом (тільки цифри) - залишаємо в Без категорії
            if p.title.isdigit() and len(p.title) >= 8:
                uncertain.append(p)
                continue

            # === АЛКОГОЛЬ ===
            if any(w in title for w in ["ром", "rum", "black bay", "блек бей"]):
                cat_id = cat_map.get("Настоянки і тд")
                reason = "Ром"
            elif any(w in title for w in ["настойк", "soplica", "сопліца"]):
                cat_id = cat_map.get("Настоянки і тд")
                reason = "Настойка"
            elif any(w in title for w in ["кроненбург", "kronenbourg", "пабстер", "pubster", "людвік", "ludwik"]):
                cat_id = cat_map.get("Пиво")
                reason = "Пиво"
            elif any(w in title for w in ["с/а дзен", "с/а дзен", "май тай", "піна колада"]):
                cat_id = cat_map.get("Слабоалкогольні напої (сидр, енергетики)")
                reason = "Слабоалкогольний напій"
            elif any(w in title for w in ["niebieska iskra", "меморіал", "memorial", "yunior"]):
                cat_id = cat_map.get("Слабоалкогольні напої (сидр, енергетики)")
                reason = "Слабоалкогольний напій"

            # === НАПОЇ ===
            elif any(w in title for w in ["пепсі", "pepsi", "pepsi black", "пепсі-кола"]):
                cat_id = cat_map.get("Солодка вода")
                reason = "Pepsi"
            elif any(w in title for w in ["мікадо", "mikado"]):
                cat_id = cat_map.get("Солодка вода")
                reason = "Мікадо"
            elif any(w in title for w in ["нект", "nectar", "сік", "juice", "мультифр"]):
                cat_id = cat_map.get("Соки")
                reason = "Сік/нектар"

            # === СОЛОДОЩІ ===
            elif any(w in title for w in ["маршмеллоу", "marshmallow", "тапка-лапка", "тучки-штучки"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Маршмеллоу"
            elif any(w in title for w in ["lovare", "lovare oriental"]):
                cat_id = cat_map.get("Кава")
                reason = "Кава Lovare"
            elif any(w in title for w in ["макарун", "macaroon", "macaroons"]):
                cat_id = cat_map.get("Печиво, вафлі, бісквіт")
                reason = "Макарун"
            elif any(w in title for w in ["печенье savoiardi", "savoiardi", "бісквіт савоярді"]):
                cat_id = cat_map.get("Печиво, вафлі, бісквіт")
                reason = "Печиво Савоярді"
            elif any(w in title for w in ["танго", "tango"]):
                cat_id = cat_map.get("Печиво, вафлі, бісквіт")
                reason = "Печиво Танго"
            elif any(w in title for w in ["кукурузк", "кукурудз паличк", "палички кукур"]):
                cat_id = cat_map.get("Снеки та чіпси")
                reason = "Кукурудзяні палички"
            elif any(w in title for w in ["поп корн", "popcorn", "попкорн", "панда поп корн"]):
                cat_id = cat_map.get("Попкорн")
                reason = "Попкорн"
            elif any(w in title for w in ["ловіта", "lovita", "желейк", "jelly", "жувасик"]):
                cat_id = cat_map.get("Желейки")
                reason = "Желейки"
            elif any(w in title for w in ["люба-буба", "luba-buba"]):
                cat_id = cat_map.get("Жуйки")
                reason = "Жуйки Люба-Буба"
            elif any(w in title for w in ["страйпс", "stripe", "bob snail", "равлик боб"]):
                cat_id = cat_map.get("Пюре фруктове")
                reason = "Фруктові снеки"
            elif any(w in title for w in ["кутя", "kutia"]):
                cat_id = cat_map.get("Крупи")
                reason = "Кутя"
            elif any(w in title for w in ["пшоно", "millet"]):
                cat_id = cat_map.get("Крупи")
                reason = "Пшоно"
            elif any(w in title for w in ["пластівці", "геркулес", "вівсянк"]):
                cat_id = cat_map.get("Крупи")
                reason = "Пластівці"
            elif any(w in title for w in ["кунжут", "sesame"]):
                cat_id = cat_map.get("Спеції")
                reason = "Кунжут"
            elif any(w in title for w in ["маскарпон", "mascarpone"]):
                cat_id = cat_map.get("Сири")
                reason = "Маскарпоне"
            elif any(w in title for w in ["намазк", "laciaty", "вершковий сир"]):
                cat_id = cat_map.get("Сири")
                reason = "Намазка сирна"
            elif any(w in title for w in ["продукт рослинно-вершковий", "ваш молочник"]):
                cat_id = cat_map.get("Маргарин")
                reason = "Продукт рослинно-вершковий"
            elif any(w in title for w in ["мед штучн", "мед штучний"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Мед штучний"
            elif any(w in title for w in ["повидл", "povidlo"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Повидло"
            elif any(w in title for w in ["начинк кондитерськ", "райдужн"]):
                cat_id = cat_map.get("Макові начинки")
                reason = "Начинка кондитерська"
            elif any(w in title for w in ["посипк на паск", "посипка"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Посипка кондитерська"
            elif any(w in title for w in ["шоковин", "червна калина"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Цукерки"
            elif any(w in title for w in ["сонечко", "спартак", "really enjoy", "солодкий спогад"]):
                cat_id = cat_map.get("Цукерки")
                reason = "Цукерки"
            elif any(w in title for w in ["круглий білий", "овальний білий", "хліб білий"]):
                cat_id = cat_map.get("Хліб")
                reason = "Хліб білий"
            elif any(w in title for w in ["косичк", "плетінк"]):
                cat_id = cat_map.get("Хліб")
                reason = "Хліб косичка"

            # === РИБА ===
            elif any(w in title for w in ["салак", "sprat", "шпрот", "кільк", "тюльк"]):
                cat_id = cat_map.get("Інша риба")
                reason = "Салака/кілька"
            elif any(w in title for w in ["міді", "mussels", "дари нептун", "нептун"]):
                cat_id = cat_map.get("Заморожена")
                reason = "Мідії/морепродукти"
            elif any(w in title for w in ["пресерв", "preserwa", "філе прямокутник", "галфіш"]):
                cat_id = cat_map.get("Інша риба")
                reason = "Пресерви рибні"

            # === М'ЯСО ===
            elif any(w in title for w in ["кості голі", "кістк"]):
                cat_id = cat_map.get("Інша м'ясна продукція")
                reason = "Кістки"
            elif any(w in title for w in ["печінк", "liver"]):
                cat_id = cat_map.get("Інша м'ясна продукція")
                reason = "Печінка"
            elif any(w in title for w in ["теляча вар", "телят"]):
                cat_id = cat_map.get("Інша м'ясна продукція")
                reason = "Телятина"
            elif any(w in title for w in ["полядвиц", "спинк", "четвертинк", "шийк"]):
                cat_id = cat_map.get("М'ясо")
                reason = "М'ясо"
            elif any(w in title for w in ["ребр", "реберц"]):
                cat_id = cat_map.get("М'ясо")
                reason = "Ребра"
            elif any(w in title for w in ["пік-нік", "picnic", "гриль ковб"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Пік-нік гриль"
            elif any(w in title for w in ["сальтисон", "sultyson"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Сальтисон"

            # === ФРУКТИ, ОВОЧІ ===
            elif any(w in title for w in ["полуниц", "strawberry"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Полуниця"
            elif any(w in title for w in ["помел", "pomelo"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Помело"
            elif any(w in title for w in ["печериц", "гриб", "mushroom"]):
                cat_id = cat_map.get("Овочі")
                reason = "Гриби"
            elif any(w in title for w in ["корнішон", "огірк марин"]):
                cat_id = cat_map.get("Овочі")
                reason = "Корнішони"

            # === ПОБУТОВА ХІМІЯ ===
            elif any(w in title for w in ["кріт для прочищення", "кріт прочищення", "sila", "сила"]):
                cat_id = cat_map.get("Хімія")
                reason = "Засіб для прочищення труб"
            elif any(w in title for w in ["порошок аріель", "ariel", "порошок тайд", "tide", "savex", "савекс"]):
                cat_id = cat_map.get("Пральні порошки")
                reason = "Пральний порошок"
            elif any(w in title for w in ["поліроль", "pronto", "для меблів"]):
                cat_id = cat_map.get("Хімія")
                reason = "Поліроль для меблів"
            elif any(w in title for w in ["сарма", "sarma", "для вікон", "повер ваш", "kalkloser"]):
                cat_id = cat_map.get("Хімія")
                reason = "Хімія"
            elif any(w in title for w in ["плин", "plyn", "людвіг", "ludwik", "гель"]):
                cat_id = cat_map.get("Хімія")
                reason = "Хімія"
            elif any(w in title for w in ["котофеїч", "засіб від пацюк", "от пацюк"]):
                cat_id = cat_map.get("Засоби захисту від комарів, сонця")
                reason = "Засоби від гризунів"
            elif any(w in title for w in ["ліпучк на мух", "bros", "липкая лента от мух"]):
                cat_id = cat_map.get("Засоби захисту від комарів, сонця")
                reason = "Липучки від мух"
            elif any(w in title for w in ["рукав д/запекания", "рукав для запікан"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Рукав для запікання"
            elif any(w in title for w in ["силіконовий паргамент", "пергамент", "freepack"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Пергамент"
            elif any(w in title for w in ["сірник", "matches"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Сірники"
            elif any(w in title for w in ["сітка для кухонної раковин", "сітка для раковин"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Сітка для раковини"
            elif any(w in title for w in ["мікрофібр", "microfiber", "тряпк", "ганчірк"]):
                cat_id = cat_map.get("Тряпки, губки, пакети сміттєві")
                reason = "Мікрофібра/тряпки"
            elif any(w in title for w in ["рушнички паперові", "рушник паперов", "sindy"]):
                cat_id = cat_map.get("Паперові вироби")
                reason = "Паперові рушники"
            elif any(w in title for w in ["салфетк волог", "салфетки влажн", "baby superfresh", "вологі серветк"]):
                cat_id = cat_map.get("Паперові вироби")
                reason = "Вологі серветки"
            elif any(w in title for w in ["медична маск", "маска медичн"]):
                cat_id = cat_map.get("Вата, бинт, диски")
                reason = "Медичні маски"
            elif any(w in title for w in ["пластир", "plaster"]):
                cat_id = cat_map.get("Вата, бинт, диски")
                reason = "Пластир"

            # === ЗАСОБИ ГІГІЄНИ ===
            elif any(w in title for w in ["лак для волосся", "леда style", "lac dla wlosow"]):
                cat_id = cat_map.get("Косметика та дезодоранти")
                reason = "Лак для волосся"
            elif any(w in title for w in ["рексуна", "rexona", "суха на чорному"]):
                cat_id = cat_map.get("Косметика та дезодоранти")
                reason = "Дезодорант"
            elif any(w in title for w in ["помада гігієнічн", "гігієнічна помада"]):
                cat_id = cat_map.get("Косметика та дезодоранти")
                reason = "Гігієнічна помада"
            elif any(w in title for w in ["презерватив", "condom", "contex", "лахот", "lahot"]):
                cat_id = cat_map.get("Аксесуари для гігієни")
                reason = "Презервативи"
            elif any(w in title for w in ["одноразові станк", "simply venus", "станок для гоління"]):
                cat_id = cat_map.get("Засоби для гоління")
                reason = "Станки для гоління"
            elif any(w in title for w in ["невидимк для волосся", "шпильк", "заколк"]):
                cat_id = cat_map.get("Аксесуари для гігієни")
                reason = "Аксесуари для волосся"
            elif any(w in title for w in ["пенз", "pumice"]):
                cat_id = cat_map.get("Аксесуари для гігієни")
                reason = "Пенза"

            # === ДЛЯ ДОМУ ===
            elif any(w in title for w in ["конверт", "envelope"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Конверти"
            elif any(w in title for w in ["коректор", "corrector", "weibo"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Коректор"
            elif any(w in title for w in ["крейд", "chalk"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Крейда"
            elif any(w in title for w in ["розмальовк", "coloring", "мандарін"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Розмальовка"
            elif any(w in title for w in ["кришк", "lid", "таламус"]):
                cat_id = cat_map.get("Посуд")
                reason = "Кришки"
            elif any(w in title for w in ["лоток", "tray"]):
                cat_id = cat_map.get("Посуд")
                reason = "Лоток"
            elif any(w in title for w in ["бамбукові паличк", "bamboo sticks"]):
                cat_id = cat_map.get("Посуд")
                reason = "Бамбукові палички"
            elif any(w in title for w in ["майк", "майка", "singlet"]):
                cat_id = cat_map.get("Пакети")
                reason = "Пакети майка"
            elif any(w in title for w in ["резинк", "гумк", "elastic"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Резинки"
            elif any(w in title for w in ["норі", "nori", "листи норі", "водорості для суші"]):
                cat_id = cat_map.get("Інша бакалія")
                reason = "Норі (водорості)"
            elif any(w in title for w in ["ролін", "rolini", "шпинат"]):
                cat_id = cat_map.get("Випічка")
                reason = "Роліни"

            # === ВСЕ ДЛЯ СВЯТА ===
            elif any(w in title for w in ["кульк", "balloon", "повітрян кульк", "gd 90", "g 90"]):
                cat_id = cat_map.get("Феєрверки, шаріки")
                reason = "Повітряні кульки"
            elif any(w in title for w in ["подарунковий конверт", "конверт подарунк"]):
                cat_id = cat_map.get("Подарункові пакети")
                reason = "Подарунковий конверт"
            elif any(w in title for w in ["мега сюрприз", "яйце сюрприз", "іграшк"]):
                cat_id = cat_map.get("Товари для дітей")
                reason = "Іграшки"

            # === НАСІННЯ ===
            elif any(w in title for w in ["матіол", "matthiola", "айстр", "цинія", "чорнобривц"]):
                cat_id = cat_map.get("Усе насіння")
                reason = "Насіння квітів"

            # === ТЮТЮН ===
            elif any(w in title for w in ["пріма", "prima", "срібна червон"]):
                cat_id = cat_map.get("Сигарети")
                reason = "Сигарети Пріма"

            # === ЗАМОРОЖЕНА ПРОДУКЦІЯ ===
            elif any(w in title for w in ["морож", "ласунк", "lasunka", "ескімо", "марите"]):
                cat_id = cat_map.get("Морозиво і десерти")
                reason = "Морозиво"

            # === МОЛОЧНІ ===
            elif any(w in title for w in ["мол.коктейль", "даноне", "danone"]):
                cat_id = cat_map.get("Молоко, йогурти, кефір")
                reason = "Молочний коктейль"

            # === ШКАРПЕТКИ ===
            elif any(w in title for w in ["носочк", "socks", "фенна", "fenna"]):
                cat_id = cat_map.get("Шкарпетки")
                reason = "Шкарпетки"

            # === КАВА, ЧАЙ ===
            elif any(w in title for w in ["суміш зеленого чаю", "магік моон", "magic moon", "чай зелений"]):
                cat_id = cat_map.get("Чай")
                reason = "Чай"
            elif any(w in title for w in ["мокачіно", "mocaccino"]):
                cat_id = cat_map.get("Кава")
                reason = "Мокачіно"

            # === ОВОЧІ ===
            elif any(w in title for w in ["листи норі", "nori"]):
                cat_id = cat_map.get("Овочі")
                reason = "Водорості норі"

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

        with open("/tmp/uncertain_products_4.txt", "w", encoding="utf-8") as f:
            for p in uncertain:
                f.write(f"{p.title}|{p.barcode or ''}|{p.id}\n")
        print(f"\nНевизначені збережено у /tmp/uncertain_products_4.txt")

    await engine.dispose()

asyncio.run(categorize())
