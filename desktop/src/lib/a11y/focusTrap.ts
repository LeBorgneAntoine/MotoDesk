/**
 * Focus management shared by every overlay layer (Modal, Panel).
 *
 * Overlays must keep the keyboard inside themselves while open and hand focus
 * back to the trigger when they close, otherwise keyboard users get dropped at
 * the top of the document.
 */

export const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), ' +
  'textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableIn(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => el.offsetParent !== null || el === document.activeElement,
  );
}

/** Move focus to the first focusable child, falling back to the container. */
export function focusFirst(container: HTMLElement | null): void {
  if (!container) return;
  const [first] = focusableIn(container);
  (first ?? container).focus();
}

/**
 * Keep Tab / Shift+Tab cycling inside `container`.
 * Returns true when the event was handled.
 */
export function trapTab(e: KeyboardEvent, container: HTMLElement | null): boolean {
  if (e.key !== "Tab" || !container) return false;

  const nodes = focusableIn(container);
  if (nodes.length === 0) {
    e.preventDefault();
    container.focus();
    return true;
  }

  const first = nodes[0];
  const last = nodes[nodes.length - 1];
  const active = document.activeElement;

  if (e.shiftKey && (active === first || active === container)) {
    e.preventDefault();
    last.focus();
    return true;
  }
  if (!e.shiftKey && active === last) {
    e.preventDefault();
    first.focus();
    return true;
  }
  return false;
}

/**
 * Remember the currently focused element so it can be restored later.
 * Call the returned function when the overlay closes.
 */
export function captureFocus(): () => void {
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  return () => previous?.focus();
}
