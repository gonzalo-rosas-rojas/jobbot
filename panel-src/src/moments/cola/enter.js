// How long a node of COLA keeps its data-enter mark after it arrives (the proposal changing, a row sliding to "tonight"): the longest of the arrival
// animations in styles/ui/cola.css (the row's, 420 ms). After it the mark is taken off so the animation can play again.
export const ENTER_MS = 420;
