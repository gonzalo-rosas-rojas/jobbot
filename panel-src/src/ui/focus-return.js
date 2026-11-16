// Where the focus goes back to when a dialog (the help, the palette) closes: the element that had it when the dialog opened.
// A dialog may open from an element that is gone by the time it closes; then the focus stays where the browser put it.

export function createFocusReturn() {
  let opener = null;
  return {
    remember() { opener = document.activeElement; },
    restore() {
      if (opener && opener.isConnected && opener.focus) opener.focus({ preventScroll: true });
      opener = null;
    },
  };
}
