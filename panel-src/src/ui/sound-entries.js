// What the command palette (Ctrl K) offers about the sound, pure and DOM-free like ui/palette-items.js: the entries are built from the player's
// status and call what the shell gave in `run`, nothing else. Only the first, "Sonido: encender / apagar" (the m key), is in the menu you see
// with nothing typed; the rest (Acciones, Fondo, Silenciar 1 hora, Silenciar hasta mañana, the working hours, the volume, Escuchar los sonidos)
// wait for a word, because the menu is for the places and the few things everybody changes, and "sonido" brings the whole family.
//
//   soundEntries({ status, run, time }) -> entries (the shape of palette-items.js: { id, group, label, sub, words, key, tag, current, run, quiet? })
//   status  player.status() (sound/player.js)
//   run     { master(), bus(id), mute(kind), work(), volume(), listen() }
//   time    (ms) -> "21:40", in the page's time zone

import { t, fmtDate } from "../i18n/index.js";
import { LEVEL_IDS } from "../sound/player.js";

const onOff = (flag) => (flag ? "on" : "off");

/** A moment as "21:40", in the page's time zone. */
export const clockTime = (ms) => fmtDate(ms, "time");

/** The words for the mode now ("acciones y fondo", "silencio hasta 21:40", ...): the indicator of the HUD and the sub-lines say the same thing. */
export function stateWords(status, time) {
  switch (status.mode) {
    case "off": return t("sound.state.off");
    case "muted": return t("sound.state.muted", { until: time(status.muteUntil) });
    case "work": return t("sound.state.work", { until: status.work.to });
    default: return t("sound.state." + status.mode);
  }
}

export function soundEntries({ status, run, time }) {
  const base = { group: "prefs", tag: "", current: false };
  const quiet = { ...base, quiet: true, key: "" };                // second-level entries: they wait for a word and have no key
  const words = t("sound.alias") + " " + t("palette.alias.sound");
  const nowPrefix = t("palette.current") + ": ";
  const nextLevel = LEVEL_IDS[(LEVEL_IDS.indexOf(status.level) + 1) % LEVEL_IDS.length];
  const entries = [
    { ...base, id: "pref:sound", label: t(status.enabled ? "sound.cmd.off" : "sound.cmd.on"), sub: nowPrefix + stateWords(status, time), words, key: "m", run: () => run.master() },
    { ...quiet, id: "pref:sound:actions", label: t("sound.cmd.actions." + (status.actions ? "off" : "on")), sub: nowPrefix + t("sound.ctl." + onOff(status.actions)), words, run: () => run.bus("actions") },
    { ...quiet, id: "pref:sound:background", label: t("sound.cmd.background." + (status.background ? "off" : "on")), sub: nowPrefix + t("sound.ctl." + onOff(status.background)), words, run: () => run.bus("background") },
  ];
  if (status.muted) {
    entries.push({ ...quiet, id: "pref:sound:unmute", label: t("sound.cmd.mute.off"), sub: t("sound.state.muted", { until: time(status.muteUntil) }), words, run: () => run.mute("off") });
  } else {
    entries.push(
      { ...quiet, id: "pref:sound:mute1h", label: t("sound.cmd.mute.hour"), sub: "", words, run: () => run.mute("hour") },
      { ...quiet, id: "pref:sound:muteTomorrow", label: t("sound.cmd.mute.tomorrow"), sub: "", words, run: () => run.mute("tomorrow") },
    );
  }
  entries.push(
    { ...quiet, id: "pref:sound:work", label: t("sound.cmd.work." + (status.work.on ? "allow" : "silence")), sub: t(status.work.on ? "sound.cmd.sub.work" : "sound.cmd.sub.workoff", { from: status.work.from, to: status.work.to }), words, run: () => run.work() },
    { ...quiet, id: "pref:sound:volume", label: t("sound.cmd.volume", { level: t("sound.volume." + nextLevel).toLowerCase() }), sub: nowPrefix + t("sound.volume." + status.level).toLowerCase(), words, run: () => run.volume() },
    { ...quiet, id: "pref:sound:listen", label: t("sound.cmd.listen"), sub: "", words, run: () => run.listen() },
  );
  return entries;
}
