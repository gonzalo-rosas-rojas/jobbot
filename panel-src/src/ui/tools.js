// The small settings the keys and the buttons share: the theme, the language, the sound and calm motion.
// One place, so `t`, `l`, `m` and the palette all do exactly the same thing and say it the same way.
//
// The theme (spec §2.2, §14 decision 2): the dial decides the sky, the windows and the sun; the sheet is Tinta unless he asks for
// Papel. So `t` is a switch between the two, not the three-way cycle of the old page ("auto" and "dark" would be the same picture).
// "auto" and "dark" both mean Tinta; the choice is kept on this machine (core/store.js) and the head script of the page reads it back.

import { t, lang, setLang, fmtDate } from "../i18n/index.js";
import { saveTheme } from "../core/store.js";

/** createTools({ toast, sound, settings }) -> { themeName, chooseTheme, chooseLang, chooseSound, chooseMotion, theme, lang, sound, motion } */
export function createTools({ toast, sound, settings }) {
  const root = document.documentElement;
  const themeName = () => root.getAttribute("data-theme") || "auto";

  function chooseTheme(name) {
    const next = name === "light" ? "light" : "dark";
    if (next === "light") root.setAttribute("data-theme", "light");
    else root.removeAttribute("data-theme");
    saveTheme(next);
    sound.play("theme", { to: next });
    toast.show(t("toast.theme", { name: t("theme." + next) }), true);
  }

  function chooseLang(next) {
    if (next === lang()) return;
    setLang(next);
    sound.play("lang", { to: next });
    toast.show(t("toast.lang", { name: t("lang." + next) }), true);
  }

  /** What the toast says once the speaker changed: off; on; or on but mute for now (working hours, or a silence he asked for) and until when. */
  function soundToast() {
    if (!sound.enabled) return t("toast.sound.off");
    const s = typeof sound.status === "function" ? sound.status() : null;
    if (s && s.mode === "work") return t("sound.toast.work", { until: s.work.to });
    if (s && s.mode === "muted") return t("sound.toast.muted", { until: fmtDate(s.muteUntil, "time") });
    return t("toast.sound.on");
  }

  /** The speaker. A key press or a click is the user gesture that lets the browser make the AudioContext. */
  function chooseSound(on) {
    sound.setEnabled(on);
    settings.set("sound", sound.enabled);
    toast.show(soundToast(), true);
    if (sound.enabled) sound.play("switch", { on: true });
  }

  /** Calm motion (no trips, no gestures) or the system's choice: the `motion` setting, which the palette offers. main.js hears it and writes data-motion. */
  function chooseMotion(mode) {
    const next = mode === "calm" ? "calm" : "auto";
    settings.set("motion", next);
    sound.play("switch", { on: next === "calm" });
    toast.show(t("toast.motion." + next), true);
  }

  return {
    themeName,
    chooseTheme,
    chooseLang,
    chooseSound,
    chooseMotion,
    theme: () => chooseTheme(themeName() === "light" ? "dark" : "light"),
    lang: () => chooseLang(lang() === "es" ? "en" : "es"),
    sound: () => chooseSound(!sound.enabled),
    motion: () => chooseMotion(settings.get("motion") === "calm" ? "auto" : "calm"),
  };
}
