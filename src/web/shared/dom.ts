type DisplayText = string | number | null | undefined;

export const node = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  text?: DisplayText,
  className?: string,
) => {
  const e = document.createElement(tag);
  if (text != null) e.textContent = String(text);
  if (className) e.className = className;
  return e;
};
export const cell = (
  row: HTMLTableRowElement,
  text?: DisplayText,
  className?: string,
) => {
  const td = node("td", text, className);
  row.append(td);
  return td;
};
export function button(
  text: string,
  action: () => void,
  className = "pointer",
) {
  const b = node("button", text, className);
  b.addEventListener("click", action);
  return b;
}
