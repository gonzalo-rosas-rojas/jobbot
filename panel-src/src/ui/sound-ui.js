// What he sees and touches of the sound: the small indicator in the HUD (ui/sound-indicator.js), the controls his buttons and the palette's entries call
// (ui/sound-controls.js) and the sheet "Escuchar los sonidos" (ui/sound-sheet.js). This module joins the three and keeps them true.
//
//   const ui = createSoundUi({ player, settings, chooseSound, toast, onToggle, root });
//   ui.indicator            the HUD button (main.js puts it in the HUD)
//   ui.sheet                { open(), close(), toggle(), isOpen() }       ui.entries()   the palette's entries about the sound
//   ui.controls             { master(), bus(id), mute(kind), work(), volume(), listen() }, what the palette's entries and the sheet's buttons call
//   ui.refresh()            works the state out again (the minute passed, the language changed)

import { onLang } from "../i18n/index.js";
import { soundEntries, clockTime } from "./sound-entries.js";
import { createSoundControls } from "./sound-controls.js";
import { createSoundIndicator } from "./sound-indicator.js";
import { createSoundSheet } from "./sound-sheet.js";

const REFRESH_MS = 15000;                                          // the end of a silence and the turn of the working day change the word without anyone touching anything

export function createSoundUi({ player, settings = null, chooseSound = null, toast = () => {}, onToggle = () => {}, root = () => document.body, timers = { setInterval: (f, ms) => setInterval(f, ms), clearInterval: (id) => clearInterval(id), setTimeout: (f, ms) => setTimeout(f, ms) } }) {
  const { controls, setSoundOn } = createSoundControls({ player, settings, chooseSound, toast, openSheet: () => sheet.open() });
  const indicator = createSoundIndicator({ player, onPress: () => sheet.toggle() });
  const { sheet, sync } = createSoundSheet({ player, controls, setSoundOn, paintIndicator: indicator.paint, onToggle, root, timers });

  // ---- keeping it true: the player tells when a choice changed; the clock tells the rest
  function refresh() {
    indicator.paint();
    sync();
  }
  player.onChange(refresh);
  timers.setInterval(refresh, REFRESH_MS);
  onLang(() => {
    indicator.forget();
    if (sheet.isOpen()) sheet.close();
    refresh();
  });
  indicator.paint();

  return {
    indicator: indicator.button,
    sheet,
    controls,
    refresh,
    entries: () => soundEntries({ status: player.status(), run: controls, time: clockTime }),
  };
}
