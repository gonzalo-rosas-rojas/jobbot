// The one question the sound asks of the page itself: is nobody looking at it? (A hidden tab plays no unsolicited sound.)

/** True while the tab is hidden; false where there is no page (Node). */
export const isHidden = () => typeof document !== "undefined" && document.hidden === true;
