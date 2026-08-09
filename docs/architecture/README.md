# 🏗️ Архітектура Torgashka — Документація

> **Версія:** 2.0.0 (Clean Architecture / DDD)  
> **Оновлено:** 2026-07-20

---

## 📋 Зміст

### Аналіз та проєктування

| Документ | Опис |
|----------|------|
| [analysis-report.md](analysis-report.md) | 📊 Звіт про аналіз поточної архітектури |
| [layers.md](layers.md) | 🏗️ Діаграма шарів (4-шарова архітектура) |
| [modules.md](modules.md) | 🧩 Карта модулів (детальний опис) |
| [refactoring-plan.md](refactoring-plan.md) | 📋 План рефакторингу (покроковий) |

### Architecture Decision Records (ADR)

| Документ | Статус | Опис |
|----------|--------|------|
| [ADR-001](../adr/ADR-001-4-layer-architecture.md) | ✅ Прийнято | 4-шарова архітектура Clean Architecture |
| [ADR-002](../adr/ADR-002-repository-pattern.md) | ✅ Прийнято | Repository Pattern |
| [ADR-003](../adr/ADR-003-domain-events.md) | ✅ Прийнято | Domain Events (слабка зв'язність) |
| [ADR-004](../adr/ADR-004-dependency-injection.md) | ✅ Прийнято | Dependency Injection Container |
| [ADR-005](../adr/ADR-005-value-objects.md) | ✅ Прийнято | Value Objects |
| [ADR-006](../adr/ADR-006-cqrs-reports.md) | ⏳ Відкладено | CQRS для звітів |

### Інше

| Документ | Опис |
|----------|------|
| [database_schema.md](../database_schema.md) | 💾 Схема бази даних |

---

## 🎯 Ключові рішення

1. **4-шарова архітектура:** Presentation → Application → Domain → Infrastructure
2. **Repository Pattern:** Інтерфейси в Domain, реалізації в Infrastructure
3. **Domain Events:** Слабка зв'язність між модулями
4. **DI Container:** Централізоване управління залежностями
5. **Value Objects:** Безпека типів, вбудована валідація

---

## 🚀 Статус міграції

- [ ] **Фаза 0:** Підготовка структури
- [ ] **Фаза 1:** Domain Layer (Entities, VOs, Interfaces)
- [ ] **Фаза 2:** Application Layer (Use Cases, DTO, Mappers)
- [ ] **Фаза 3:** Infrastructure Layer (Repositories, DI, Events)
- [ ] **Фаза 4:** Presentation Layer (оновлення роутерів)
- [ ] **Фаза 5:** Тестування
- [ ] **Фаза 6:** Міграція та деплой

---

> **Створено:** System Architect Agent (AEGIS v3)
