// The small indicator of the HUD that says in a word what the sound is doing now (off, actions, actions and background, silent until 21:40, mute for
// working hours). It draws again only when the word changes. Pressing it is the opener's business (the sheet, ui/sound-sheet.js).
//
//   createSoundIndicator({ player, onPress }) -> { button, paint(), forget() }     forget(): the next paint draws even if nothing changed (a new language)

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { icon } from "./icons.js";
import { clockTime, stateWords } from "./sound-entries.js";

const ICON_OF_MODE = { work: "clock", off: "volume-x", muted: "volume-x", none: "volume-x" };

export function createSoundIndicator({ player, onPress }) {
  const indicatorText = h("span", { class: "hud-sound-text" });
  const indicatorIcon = h("span", { class: "hud-sound-icon" });
  const indicator = h("button", { class: "hud-sound label", type: "button", "aria-haspopup": "dialog", onclick: onPress }, indicatorIcon, indicatorText);
  let shownKey = "";

  function paint() {
    const s = player.status();
    const words = stateWords(s, clockTime);
    const key = `${s.mode}|${words}|${s.volume}`;
    if (key === shownKey) return;
    shownKey = key;
    indicator.dataset.mode = s.mode;
    indicatorText.textContent = words;
    indicatorIcon.replaceChildren(icon(ICON_OF_MODE[s.mode] || "volume-2"));
    indicator.setAttribute("aria-label", s.mode === "work" ? t("sound.hud.aria.work", { until: s.work.to }) : t("sound.hud.aria", { state: words }));
    indicator.title = t("sound.hud.title", { state: words });
  }

  return { button: indicator, paint, forget() { shownKey = ""; } };
}
