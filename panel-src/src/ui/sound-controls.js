// What the sound's controls do (the sheet's buttons and the palette's entries call these): the sound on or off, the two buses, the silences, the working
// hours, the volume. Each says what it did through the toast; the sheet that shows them is ui/sound-sheet.js.
//
//   createSoundControls({ player, settings, chooseSound, toast, openSheet }) -> { controls, setSoundOn }
//   controls     { master(), bus(id), mute(kind), work(), volume(), listen() }
//   setSoundOn   turns the sound on or off and remembers it, without a word (the sheet does it when a sound is asked for with the sound off)
//   chooseSound  the shell's own way (ui/tools.js: it also says it in the toast); without it the controls go to the player directly

import { t } from "../i18n/index.js";
import { HOUR_MS } from "../core/time.js";
import { clockTime } from "./sound-entries.js";


export function createSoundControls({ player, settings, chooseSound, toast, openSheet }) {
  const say = (key, vars) => toast(t(key, vars), true);

  /** Turns the sound on or off and remembers the choice. */
  function setSoundOn(on) {
    player.setEnabled(on);
    if (settings) settings.set("sound", player.enabled);
  }

  function enable(on) {
    if (chooseSound) chooseSound(on);
    else setSoundOn(on);
  }

  const controls = {
    master: () => enable(!player.enabled),
    bus(id) {
      const next = !player.status()[id];
      player.setBus(id, next);
      say(`sound.toast.${id}.${next ? "on" : "off"}`);
    },
    mute(kind) {
      if (kind === "off") { player.unmute(); say("sound.toast.unmute"); return; }
      const until = kind === "tomorrow" ? player.muteUntilTomorrow() : player.muteFor(HOUR_MS);
      say("sound.toast.mute", { until: clockTime(until) });
    },
    work() {
      const allow = player.status().work.on;
      player.setWork(!allow);
      say(allow ? "sound.toast.work.allow" : "sound.toast.work.silence");
    },
    volume() {
      const level = player.cycleLevel();
      say("sound.toast.volume", { level: t("sound.volume." + level).toLowerCase() });
      player.preview("switch");
    },
    listen: () => openSheet(),
  };

  return { controls, setSoundOn };
}
