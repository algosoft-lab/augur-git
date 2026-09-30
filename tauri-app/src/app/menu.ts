/** Browser event used to route in-window menu selections through the app handler. */
export const MENU_ACTION_EVENT = 'augur:menu-action';

/** Keep native and in-window menus on the same action path. */
export function dispatchMenuAction(id: string): void {
  window.dispatchEvent(new CustomEvent(MENU_ACTION_EVENT, { detail: id }));
}
