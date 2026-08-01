/**
 * Утиліти для обробки HTML перед друком / скріншотом (html2canvas).
 *
 * Проблема: `extractBodyContent` викидав <head> разом з усім CSS
 * (.grid-container, .tag-cell, шрифти) → html2canvas (Tauri-друк термо)
 * рендерив без стилів → ламаний вигляд.
 *
 * Рішення: `extractBodyWithStyles` зберігає <style> з <head> і вставляє їх
 * на початок вмісту body — html2canvas отримує CSS (flex, розміри, шрифти).
 */

/**
 * Витягує всі <style>...</style> з <head> та повертає контент <body>
 * РАЗОМ зі стилями (стилі на початку).
 *
 * @param html — повний HTML-документ (напр. згенерований шаблон)
 * @returns `<style>...</style>` + контент body
 */
export function extractBodyWithStyles(html: string): string {
  // 1. Знайти всі <style>...</style> (зазвичай у <head>)
  const styleMatches = html.match(/<style[^>]*>[\s\S]*?<\/style>/gi) || [];
  const styles = styleMatches.join('\n');

  // 2. Витягти контент body
  const bodyMatch = html.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  let bodyContent: string;
  if (bodyMatch) {
    bodyContent = bodyMatch[1];
  } else {
    // Fallback: немає тега <body> — прибираємо DOCTYPE/html/head
    let clean = html.replace(/<!DOCTYPE[^>]*>/gi, '');
    clean = clean.replace(/<\/?html[^>]*>/gi, '');
    clean = clean.replace(/<head[^>]*>[\s\S]*?<\/head>/gi, '');
    bodyContent = clean.trim();
  }

  // 3. Повернути стилі + контент body
  return styles ? `${styles}\n${bodyContent}` : bodyContent;
}

/**
 * Витягує лише контент <body> (без стилів).
 * Залишено для сумісності; для html2canvas використовуйте extractBodyWithStyles.
 */
export function extractBodyContent(html: string): string {
  const bodyMatch = html.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  if (bodyMatch) return bodyMatch[1];
  let clean = html.replace(/<!DOCTYPE[^>]*>/gi, '');
  clean = clean.replace(/<\/?html[^>]*>/gi, '');
  clean = clean.replace(/<head[^>]*>[\s\S]*?<\/head>/gi, '');
  return clean.trim();
}
