import React from 'react';

/**
 * Надійне виділення вмісту поля при фокусі.
 *
 * Проблема: простий `onFocus → select()` нестабільний — браузер після кліку
 * обробляє `mouseup`/`click` і скидає виділення, ставлячи курсор у місце кліку
 * («на секунду виділилось і все»).
 *
 * Рішення: якщо фокус встановлено кліком (mousedown на несфокусованому полі),
 * наступний `mouseup` перехоплюється через `preventDefault()` — виділення
 * залишається. Повторний клік у вже сфокусованому полі працює звичайно
 * (курсор ставиться в місце кліку для точного редагування).
 */

const SELECTING_ATTR = 'data-select-on-focus';

/** Викликати в `onMouseDown` перед focus. */
export function markSelectOnMouseDown(e: React.MouseEvent<HTMLInputElement>) {
  const el = e.currentTarget;
  if (document.activeElement !== el) {
    el.setAttribute(SELECTING_ATTR, '1');
  }
}

/** Викликати в `onMouseUp` — блокує скидання виділення після focus-кліку. */
export function preventSelectionClearOnMouseUp(e: React.MouseEvent<HTMLInputElement>) {
  const el = e.currentTarget;
  if (el.getAttribute(SELECTING_ATTR) === '1') {
    e.preventDefault();
    el.removeAttribute(SELECTING_ATTR);
  }
}

/** Викликати в `onFocus` — виділяє вміст (крім паролів). */
export function selectAllOnFocus(e: React.FocusEvent<HTMLInputElement>) {
  if (e.currentTarget.type !== 'password') {
    e.currentTarget.select();
  }
}
