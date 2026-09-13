# Тюнінг пошуку products (хаб <hub-host>:5432, db pos_system) — 2026-09-13

## Що зроблено (єдиний DDL — CREATE INDEX)
```
CREATE INDEX CONCURRENTLY IF NOT EXISTS ix_products_title_trgm   ON products USING gin (title   gin_trgm_ops);  -> CREATE INDEX
CREATE INDEX CONCURRENTLY IF NOT EXISTS ix_products_barcode_trgm ON products USING gin (barcode gin_trgm_ops);  -> CREATE INDEX
```
Обидва `indisvalid=t`, INVALID-індексів 0, рядків даних не змінено (products = 4014 до і після).
Розмір: ix_products_title_trgm = 840 kB (860160 B), ix_products_barcode_trgm = 224 kB (229376 B).
pg_total_relation_size('products'): 1928 kB -> 2992 kB (+1064 kB індексів).

## До / Після (EXPLAIN ANALYZE, Execution Time)
| Запит | До | Після | План після |
|---|---|---|---|
| `count(*) WHERE title ILIKE '%молоко%'` | 59.9 мс Seq Scan | **2.49 / 2.94 / 3.25 мс** | Bitmap Index Scan on ix_products_title_trgm |
| репозиторний (title-only) + count() OVER () + ORDER BY lower(title) LIMIT 50 | 77.2 мс | **2.24 / 4.02 / 4.51 мс** | Bitmap Index Scan on ix_products_title_trgm |
| репозиторний (title ILIKE OR barcode ILIKE) | 77.2 мс | 80.1 / 83.0 / 86.3 мс (Seq Scan, DEFAULT) | Seq Scan |
| те саме з `SET random_page_cost=1.1` (сесійно) | — | **3.77 / 3.80 / 5.20 мс** | BitmapOr(title_trgm, barcode_trgm) |
| `count(*) WHERE barcode ILIKE '%482%'` | 27.0 мс | 21.4 мс (Seq Scan — 2533/4014 рядків = 63%, індекс недоцільний) | Seq Scan |

## Причина, чому OR-форма не бере індекс (root cause)
`random_page_cost = 4` (default, source=default). BitmapOr+heap коштує 187.59, Seq Scan 180.21 ->
планер обирає Seq Scan. Фактично bitmap швидший у ~20x.
Вузьке місце — heap-частина: 34 сторінки * random_page_cost=4 = 136.

## Семантика не змінилась (md5 ordered id-list BEFORE == AFTER)
| термін | count | md5 |
|---|---|---|
| title 'молоко' | 51 | 4188f4eb1c1b3f0ec57e870b2b0f6311 |
| title 'мол' | 143 | f81b785d1d9150b2e9f015e6f4e0b0d6 |
| title 'МОЛОКО' | 51 | 4188f4eb1c1b3f0ec57e870b2b0f6311 |
| title 'a' | 641 | 299b8655a7768a7eadb63095f70642cd |
| title '4' | 418 | 9b91d5098c60e5ae6216f2212227a79d |
| title ';;;' | 0 | - |
| title 'шт' | 140 | 6b2f6141678c9e304f32c42e49a2eb65 |
| repo LIMIT 50 | 50 | 6879331ef6df04823b621d0e7b81e4b0 |
| barcode '4' | 3280 | a5408c5145bc8895052f1f63e6210e62 |
| barcode '77' | 456 | 7e69de168f9a2f961a017eafa175ee57 |
| barcode '482' | 2533 | 32b6338cf02eaa53b9fa29f6d18d6443 |
