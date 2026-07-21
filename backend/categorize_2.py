"""
Другий етап категоризації товарів.
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

            # === ПОБУТОВА ХІМІЯ ===
            if any(w in title for w in ["гель для прання", "гель для стирки", "пральний порошок", "порошок праль", "wash free", "tide", "персил", "persil", "пральн"]):
                cat_id = cat_map.get("Пральні порошки")
                reason = "Пральні порошки"
            elif any(w in title for w in ["мило", "soap", "safeguard", "мильничк", "мочалк"]):
                cat_id = cat_map.get("Мило, мильничка, мочалки")
                reason = "Мило, мильничка, мочалки"
            elif any(w in title for w in ["одноразовий посуд", "тарілка однораз", "вилка однораз", "ложка однораз", "ніж однораз", "стакан однораз", "склянка однораз"]):
                cat_id = cat_map.get("Одноразовий посуд")
                reason = "Одноразовий посуд"
            elif any(w in title for w in ["паперові вироби", "паперові рушник", "туалетний папір", "серветк", "паперові хуст", "носовичк"]):
                cat_id = cat_map.get("Паперові вироби")
                reason = "Паперові вироби"
            elif any(w in title for w in ["рукавиці", "рукавичк", "перчатк"]):
                cat_id = cat_map.get("Рукавиці")
                reason = "Рукавиці"
            elif any(w in title for w in ["тряпк", "ганчірк", "губк", "пакети сміттєві", "сміттєві пакети", "пакет для сміття"]):
                cat_id = cat_map.get("Тряпки, губки, пакети сміттєві")
                reason = "Тряпки, губки, пакети сміттєві"
            elif any(w in title for w in ["засіб захисту від комар", "засіб від комар", "від комах", "fumitox", "раптор", "mosquito", "антикомарин", "спіраль від комар", "фумігатор", "лосьйон від комар", "крем від комар", "захист від сонця", "сонцезахисн", "spf", "sunscreen"]):
                cat_id = cat_map.get("Засоби захисту від комарів, сонця")
                reason = "Засоби захисту від комарів, сонця"
            elif any(w in title for w in ["хімія", "засіб для чищення", "миючий", "cleaner", "рідина для миття", "засіб для миття", "відбілювач", "плямовивідник", "vanish", "tytan", "wirek", "denkmit", "dr.beckmann", "шантаклєр", "chanteclar", "грасаторе", "gransatore", "pronto", "антипил", "освіжувач", "освежитель", "кондиціонер для білизни", "пом якшувач", "ополіскувач", "засіб для скла", "склоочисник", "миття вікон", "миття туалет", "миття кухн", "сантехнік", "clean"]):
                cat_id = cat_map.get("Хімія")
                reason = "Хімія"
            elif any(w in title for w in ["товари для кухні", "кухонне приладдя", "кухонні прилад", "скатертин", "фольг", "харчова плівк", "пергамент", "папір для випічки", "рукав для запікан", "пакет для замороз", "пакет фасувальн", "мішечк", "зубочистк", "шпажк"]):
                cat_id = cat_map.get("Товари для кухні")
                reason = "Товари для кухні"

            # === МОЛОЧНІ ПРОДУКТИ ТА ЯЙЦЯ ===
            elif any(w in title for w in ["молок", "йогурт", "кефір", "ряжанк", "простокваш", "сніжок", "milk", "yogurt", "kefir"]):
                cat_id = cat_map.get("Молоко, йогурти, кефір")
                reason = "Молоко, йогурти, кефір"
            elif any(w in title for w in ["сир", "syr", "sire", "cheese", "бринз", "моцарел", "пармезан", "гауда", "едем", "маасдам", "російський сир", "голландський", "ковбасний сир", "плавлений сир", "сирний продукт", "янтар", "вершковий сир"]):
                cat_id = cat_map.get("Сири")
                reason = "Сири"
            elif any(w in title for w in ["сметан", "smetana"]):
                cat_id = cat_map.get("Сметана")
                reason = "Сметана"
            elif any(w in title for w in ["яйц", "egg", "яєчк"]):
                cat_id = cat_map.get("Яйця")
                reason = "Яйця"
            elif any(w in title for w in ["згущен", "згущене молоко", "condensed"]):
                cat_id = cat_map.get("Згущене молоко")
                reason = "Згущене молоко"
            elif any(w in title for w in ["маргарин", "margarine", "спред"]):
                cat_id = cat_map.get("Маргарин")
                reason = "Маргарин"
            elif any(w in title for w in ["масло вершк", "масло селян", "масло солодк", "butter", "олійк"]):
                cat_id = cat_map.get("Масло")
                reason = "Масло"

            # === М'ЯСО ===
            elif any(w in title for w in ["курк", "kurka", "chicken", "гомілк", "стегн", "філе кур", "крильц", "тушк кур", "куряч"]):
                cat_id = cat_map.get("Куряче")
                reason = "Куряче"
            elif any(w in title for w in ["фарш", "farce"]):
                cat_id = cat_map.get("Фарш")
                reason = "Фарш"
            elif any(w in title for w in ["шашлик", "shashlyk"]):
                cat_id = cat_map.get("Шашлик")
                reason = "Шашлик"
            elif any(w in title for w in ["м'ясо", "мясо", "свинин", "ялович", "телят", "баранин", "корейк", "вирізк", "шийк", "лопатк", "грудинк", "бекон", "сало", "шпик", "шинк", "окіст", "рулет м'ясн", "рулет мясн", "ковбас", "сосис", "сардельк", "шпондер"]):
                cat_id = cat_map.get("Інша м'ясна продукція")
                reason = "Інша м'ясна продукція"

            # === КОВБАСИ ===
            elif any(w in title for w in ["ковбас", "sausage", "салям", "salami", "сервелат", "краківськ", "варена ковб", "копчен ковб", "напівкопчен", "ліверн", "кров'ян", "кровян"]):
                cat_id = cat_map.get("Ковбаси")
                reason = "Ковбаси"
            elif any(w in title for w in ["сардельк", "сосис"]):
                cat_id = cat_map.get("Сардельки, сосиски")
                reason = "Сардельки, сосиски"

            # === РИБА ===
            elif any(w in title for w in ["оселедець", "herring", "селед", "оселед"]):
                cat_id = cat_map.get("Оселедець")
                reason = "Оселедець"
            elif any(w in title for w in ["червона риба", "лосос", "salmon", "форель", "trout", "сьомг"]):
                cat_id = cat_map.get("Червона риба")
                reason = "Червона риба"
            elif any(w in title for w in ["копчен", "smoked", "шпрот"]):
                cat_id = cat_map.get("Копчена")
                reason = "Копчена"
            elif any(w in title for w in ["ікра", "caviar"]):
                cat_id = cat_map.get("Ікра")
                reason = "Ікра"
            elif any(w in title for w in ["заморожен риб", "риба заморож", "морськ", "анчоус", "тріск", "cod", "хек", "минтай", "pollock", "путасу", "мойв", "capelin", "камбала", "flounder", "окун", "perch", "судак", "pike", "щук", "карась", "короп", "carp", "товстолоб", "сом", "catfish"]):
                cat_id = cat_map.get("Заморожена")
                reason = "Заморожена риба"
            elif any(w in title for w in ["риб", "fish"]):
                cat_id = cat_map.get("Інша риба")
                reason = "Інша риба"

            # === ФРУКТИ, ОВОЧІ ===
            elif any(w in title for w in ["овоч", "помідор", "tomato", "огірк", "cucumber", "капуст", "cabbage", "моркв", "carrot", "буряк", "beet", "цибул", "onion", "часник", "garlic", "картопл", "potato", "кабачк", "zucchini", "баклаж", "eggplant", "перець", "pepper", "редис", "radish", "салат", "lettuce", "зелень", "кріп", "петрушк", "шпінат", "spinach", "гриб", "mushroom", "шампіньйон", "брокол", "цвітна капуст", "кукурудз", "горох", "квасол", "спарж"]):
                cat_id = cat_map.get("Овочі")
                reason = "Овочі"
            elif any(w in title for w in ["фрукт", "яблук", "apple", "груш", "pear", "слив", "plum", "вишн", "cherry", "черешн", "абрикос", "apricot", "персик", "peach", "нектар", "nectarine", "банан", "banana", "апельсин", "orange", "мандарин", "tangerine", "лимон", "lemon", "лайм", "lime", "грейпфрут", "grapefruit", "авокад", "avocado", "ківі", "kiwi", "ананас", "pineapple", "манго", "mango", "виноград", "grape", "кавун", "watermelon", "дин", "melon", "гранат", "pomegranate", "хурм", "persimmon", "айв", "quince"]):
                cat_id = cat_map.get("Фрукти")
                reason = "Фрукти"

            # === ЗАМОРОЖЕНА ПРОДУКЦІЯ ===
            elif any(w in title for w in ["морозив", "ice cream", "десерт заморож", "пломбір", "ескімо"]):
                cat_id = cat_map.get("Морозиво і десерти")
                reason = "Морозиво і десерти"
            elif any(w in title for w in ["напівфабрикат", "пельмен", "вареник", "галушк", "котлет", "нагетс", "nuggets", "чебурек", "хачапур", "піц", "pizza", "дерун", "млинець", "bliny", "голубц", "фарширован"]):
                cat_id = cat_map.get("Напівфабрикати")
                reason = "Напівфабрикати"
            elif any(w in title for w in ["заморожен овоч", "заморожені овоч", "крабові паличк", "крабові пал"]):
                cat_id = cat_map.get("Заморожені овочі, крабові палички")
                reason = "Заморожені овочі, крабові палички"

            # === ХЛІБ ===
            elif any(w in title for w in ["піддністрянськ", "piddnistrianskyi"]):
                cat_id = cat_map.get("Піддністрянський хліб")
                reason = "Піддністрянський хліб"
            elif any(w in title for w in ["березин"]):
                cat_id = cat_map.get("Хліб Березина")
                reason = "Хліб Березина"
            elif any(w in title for w in ["калуш"]):
                cat_id = cat_map.get("Хліб Калуш")
                reason = "Хліб Калуш"
            elif any(w in title for w in ["стасюк", "stasiuk"]):
                cat_id = cat_map.get("Хліб Пекарня Стасюка")
                reason = "Хліб Пекарня Стасюка"
            elif any(w in title for w in ["хліб", "bread", "батон", "булк", "булочк", "паляниц", "багет", "baton", "плетінк", "калач", "рогалик", "крендель", "бублик", "сушк", "баранк", "хлібець", "pikkolo", "хлібці"]):
                cat_id = cat_map.get("Хліб")
                reason = "Хліб"

            # === ВИПІЧКА ===
            elif any(w in title for w in ["випічк", "тістечк", "кекс", "мафін", "muffin", "круасан", "croissant", "пончик", "донат", "булочк", "пиріж", "пирог", "ватрушк", "слойк", "листков", "тісто", "dough"]):
                cat_id = cat_map.get("Випічка")
                reason = "Випічка"
            elif any(w in title for w in ["лаваш", "lavash", "тортил", "tortilla", "піт"]):
                cat_id = cat_map.get("Лаваш")
                reason = "Лаваш"

            # === ЗАСОБИ ГІГІЄНИ ===
            elif any(w in title for w in ["зубн паст", "зубна паст", "toothpaste", "аквафреш", "aquafresh", "colgate", "blend-a-med", "blendamed", "oral-b", "splat", "parodontax", "lacalut", "elmex", "зубн щітк", "toothbrush", "зубна нитк", "floss", "ополіскувач рот", "mouthwash"]):
                cat_id = cat_map.get("Зубні пасти")
                reason = "Зубні пасти"
            elif any(w in title for w in ["прокладк", "pad", "naturella", "always", "libresse", "molped", "discreet", "daily", "ultra", "normal", "night"]):
                cat_id = cat_map.get("Прокладки")
                reason = "Прокладки"
            elif any(w in title for w in ["фарба для волосся", "фарба волос", "hair color", "hair dye", "palette", "garnier", "loreal", "l'oreal", "schwarzkopf", "wellaton", "estel", "капус", "capus"]):
                cat_id = cat_map.get("Фарба для волосся")
                reason = "Фарба для волосся"
            elif any(w in title for w in ["косметик", "дезодорант", "антиперспірант", "deodorant", "nivea", "rexona", "dove", "old spice", "axe", "adidas", "шампун", "shampoo", "schauma", "head & shoulders", "head and shoulders", "pantene", "elseve", "кондиціонер волос", "бальзам волос", "гель для душ", "shower gel", "dermomed", "лосьйон", "lotion", "крем для тіл", "body cream", "подарунковий набір косметик", "подарунковий набір гігієн"]):
                cat_id = cat_map.get("Косметика та дезодоранти")
                reason = "Косметика та дезодоранти"
            elif any(w in title for w in ["вата", "бинт", "диск", "cotton", "паличк ват", "ватн паличк", "ватний диск", "спонж", "марл", "відріз марл"]):
                cat_id = cat_map.get("Вата, бинт, диски")
                reason = "Вата, бинт, диски"
            elif any(w in title for w in ["засіб для гоління", "голінн", "shaving", "shave", "gillete", "gillette", "blue", "лез", "blade", "бритв", "піна для гоління", "гель для гоління", "крем для гоління"]):
                cat_id = cat_map.get("Засоби для гоління")
                reason = "Засоби для гоління"
            elif any(w in title for w in ["аксесуар для гігієн", "вухочистк", "щітк", "пилочк", "ніжниці", "манікюр", "пемз", "пінцет"]):
                cat_id = cat_map.get("Аксесуари для гігієни")
                reason = "Аксесуари для гігієни"
            elif any(w in title for w in ["памперс", "pampers", "підгузк", "diaper", "huggies"]):
                cat_id = cat_map.get("Памперси")
                reason = "Памперси"

            # === ТЮТЮН ===
            elif any(w in title for w in ["сигарет", "cigarette", "chesterfield", "sobranie", "london", "blue", "original", "марльборо", "marlboro", "winston", "parliament", "kent", "camel", "lucky", "bond", "ld", "прилук", "compliment"]):
                cat_id = cat_map.get("Сигарети")
                reason = "Сигарети"
            elif any(w in title for w in ["запальничк", "lighter", "duum", "запалка"]):
                cat_id = cat_map.get("Запальнички")
                reason = "Запальнички"

            # === ЗООТОВАРИ ===
            elif any(w in title for w in ["корм для тварин", "корм для кот", "корм для собак", "корм для риб", "whiskas", "kitekat", "friskies", "pedigree", "chappi", "cesar", "біле меню", "bile menu", "sheba", "gourmet", "perfect fit", "hill's", "royal canin", "purina", "акваріум", "териріум", "наповнювач туалет", "dog", "cat", "pet"]):
                cat_id = cat_map.get("Корм для тварин")
                reason = "Корм для тварин"

            # === ДЛЯ ДОМУ ===
            elif any(w in title for w in ["догляд за взутт", "губка для взутт", "щітка для взутт", "крем для взутт", "блискавк", "шнурівк", "взутт"]):
                cat_id = cat_map.get("Догляд за взуттям")
                reason = "Догляд за взуттям"
            elif any(w in title for w in ["електрик", "батар", "battery", "лампочк", "лампа", "light", "подовжув", "розетк", "вимикач", "патрон", "провід", "кабель", "ізоляц", "скотч", "енерджайзер", "energizer", "duracell"]):
                cat_id = cat_map.get("Електрика")
                reason = "Електрика"
            elif any(w in title for w in ["канцеляр", "ручк", "pen", "олівець", "pencil", "зошит", "notebook", "альбом", "фломастер", "маркер", "лінійк", "ruler", "ластик", "eraser", "точилк", "sharpener", "клей", "glue", "ножиці", "scissors", "степлер", "дірокол", "папк", "folder", "файл", "обкладинк", "швидкозшив", "скорозшив", "папір офіс", "папір для принтер", "папка", "тетрад"]):
                cat_id = cat_map.get("Канцелярія")
                reason = "Канцелярія"
            elif any(w in title for w in ["клей", "glue", "момент", "pva", "пва", "суперклей"]):
                cat_id = cat_map.get("Клеї")
                reason = "Клеї"
            elif any(w in title for w in ["посуд", "тарілк", "plate", "чашк", "cup", "склянк", "glass", "кружк", "mug", "миск", "bowl", "салатник", "блюдц", "чайник", "kettle", "каструл", "pan", "сковорід", "frying", "деко", "baking", "ніж кухон", "knife", "дошка оброб", "cutting board", "ложк", "spoon", "виделк", "fork", "ополоник", "шумівк", "тертк", "grater", "сит", "друшл", "коланд", "валік", "rolling", "качалк", "підставк", "coaster", "пляшк", "bottle", "контейнер", "container", "хлібниц", "цукорниц", "сільничк", "перечниц", "графин", "кухоль", "келих", "чарк", "стопк", "фужер", "піал"]):
                cat_id = cat_map.get("Посуд")
                reason = "Посуд"
            elif any(w in title for w in ["рукоділл", "пряж", "нитк", "голк", "спиц", "гачок", "бісер", "вишивк", "embroidery", "канв", "пяльц", "фетр", "foam", "декор"]):
                cat_id = cat_map.get("Рукоділля")
                reason = "Рукоділля"
            elif any(w in title for w in ["текстиль", "плед", "ковдр", "blanket", "подушк", "pillow", "рушник", "towel", "скатерт", "tablecloth", "фіранк", "curtain", "штор", "покривал", "наматрац", "матрац", "постільн", "простирадл", "наволочк", "підковдр"]):
                cat_id = cat_map.get("Текстиль")
                reason = "Текстиль"
            elif any(w in title for w in ["шкарпетк", "socks", "носок", "гольф", "колготк", "tights", "панчох"]):
                cat_id = cat_map.get("Шкарпетки")
                reason = "Шкарпетки"

            # === ВСЕ ДЛЯ СВЯТА ===
            elif any(w in title for w in ["листівк", "card", "postcard", "вітальн"]):
                cat_id = cat_map.get("Листівки")
                reason = "Листівки"
            elif any(w in title for w in ["подарунковий пакет", "подарунков пак", "подарунковий міш", "подарунков короб", "подарунковий набір", "gift"]):
                cat_id = cat_map.get("Подарункові пакети")
                reason = "Подарункові пакети"
            elif any(w in title for w in ["феєрверк", "шарік", "повітрян кульк", "balloon", "святков", "гірлянд", "хлопавк", "бенгал", "салют", "петард"]):
                cat_id = cat_map.get("Феєрверки, шаріки")
                reason = "Феєрверки, шаріки"

            # === ЛАМПАДКИ, СВІЧКИ ===
            elif any(w in title for w in ["лампадк", "lampada"]):
                cat_id = cat_map.get("Лампадки")
                reason = "Лампадки"
            elif any(w in title for w in ["свічк", "candle", "восков"]):
                cat_id = cat_map.get("Свічки")
                reason = "Свічки"

            # === СОУСИ ТА СПЕЦІЇ ===
            elif any(w in title for w in ["кетчуп", "ketchup"]):
                cat_id = cat_map.get("Кетчуп")
                reason = "Кетчуп"
            elif any(w in title for w in ["майонез", "mayonnaise"]):
                cat_id = cat_map.get("Майонез")
                reason = "Майонез"
            elif any(w in title for w in ["сіль", "salt", "цукор", "sugar", "пісок", "рафінад"]):
                cat_id = cat_map.get("Сіль, цукор")
                reason = "Сіль, цукор"
            elif any(w in title for w in ["соус", "sauce", "гірчиц", "mustard", "хрін", "adjika", "аджик", "соєвий", "soy", "теріякі", "teriyaki", "томатн паст", "tomato paste", "ткемал", "барбекю", "bbq", "тартар", "tartar", "пест", "pesto"]):
                cat_id = cat_map.get("Соуси")
                reason = "Соуси"
            elif any(w in title for w in ["спеці", "spice", "приправ", "seasoning", "лавров", "перець", "pepper", "паприк", "paprika", "кориц", "cinnamon", "ваніл", "vanilla", "імбир", "ginger", "куркум", "turmeric", "коріандр", "coriander", "кмин", "cumin", "кардамон", "cardamom", "гвоздик", "clove", "мускат", "nutmeg", "шафран", "saffron", "розмарин", "rosemary", "чебрець", "thyme", "базилік", "basil", "орегано", "oregano", "майоран", "mejorana", "естрагон", "tarragon", "лавр", "хмелі-сунелі", "бульйон", "cube", "кубик", "вегет", "vegeta", "маггі", "maggi", "глютамат", "підсилюв", "розпушув", "baking", "крохмал", "starch", "желатин", "gelatin", "амоній", "лимонн кислот", "citric"]):
                cat_id = cat_map.get("Спеції")
                reason = "Спеції"

            # === ТОВАРИ ДЛЯ ДІТЕЙ ===
            elif any(w in title for w in ["аксесуар для телефон", "чохол телефон", "чохол для телефон", "скло телефон", "зарядк", "power bank", "навушник", "headphone", "кабель usb", "usb", "micro usb", "type-c", "тримач телефон", "підставка телефон", "самоклейк", "попсокет", "pop socket"]):
                cat_id = cat_map.get("Аксесуари для телефону")
                reason = "Аксесуари для телефону"

            # === ПАКЕТИ ===
            elif any(w in title for w in ["пакет", "bag", "super luxe", "сміттєв"]):
                cat_id = cat_map.get("Пакети")
                reason = "Пакети"

            # === НАСІННЯ ===
            elif any(w in title for w in ["насінн", "seed", "ядра соняшник", "ядро соняшник", "almaz", "сан санич"]):
                cat_id = cat_map.get("Усе насіння")
                reason = "Усе насіння"

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

        with open("/tmp/uncertain_products_2.txt", "w", encoding="utf-8") as f:
            for p in uncertain:
                f.write(f"{p.title}|{p.barcode or ''}|{p.id}\n")
        print(f"\nНевизначені збережено у /tmp/uncertain_products_2.txt")

    await engine.dispose()

asyncio.run(categorize())
