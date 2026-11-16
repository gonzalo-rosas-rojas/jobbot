// How the drawer does things. Every button that changes data goes through `perform`: the button is off while the request
// runs, the sounds that go with an action play ("confirm" when it goes out, then the cue of what it did, "error" when it
// failed), the server's own words reach the toast through `session.act`, approve and confirm show their short "Aprobada" /
// "Anotada" for a moment, and then the application is read again so the drawer says what is true now. A permanent action
// (approve, confirm, withdraw, reject, accept) is a two-step button: the first press arms it and says so, the second acts.

import { ARM_MAX_MS, twoStep } from "../../core/dom.js";
import { t } from "../../i18n/index.js";

const SYNC_SLACK_MS = 40; // past ARM_MAX_MS, so the timer of `twoStep` has already disarmed the button when the mark is re-synced
const STAGED_MS = 600; // how long "Aprobada" stays on the button before the list closes over it

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function sync(button, label) {
  button.toggleAttribute("data-armed", !button.classList.contains("sent") && button.textContent !== label);
}

/**
 * The click handler of a permanent button: `core/dom` twoStep, plus a `data-armed` mark the styles can show.
 * `sound` plays "arm" on the first press and `announce` says it aloud (the shell's live region); each button keeps the ones of
 * the drawing it was made in, so a button of an older drawing never uses what a newer one handed over.
 */
export function armed(label, run, { sound = null, announce = null } = {}) {
  const step = twoStep(label, run, { onArm: () => { if (sound) sound.play("arm"); } });      // the second press sounds as the button's own "confirm" (perform)
  return (event) => {
    const button = event.currentTarget;
    const before = button.textContent;
    step(event);
    const armedNow = button.textContent !== before && button.textContent === t("act.sure");
    if (armedNow && typeof announce === "function") announce(t("act.sure")); // armed just now: say so
    sync(button, label);
    setTimeout(() => sync(button, label), ARM_MAX_MS + SYNC_SLACK_MS);
  };
}

/**
 * Sends `body` to `path` for `button`, then re-reads the open application.
 *   cue      "approve" | "undo": the sound of what the action means, on top of press and release
 *   staged   "act.approved" | "act.noted": the word the button shows for a moment once it worked
 *   onOk     runs when it worked, before anything is read again
 *   onFail   runs with the refusal (`{ ok, data }`) before the drawer is read again
 * Resolves to `session.act`'s answer ({ ok, status, data, message }).
 */
export async function perform(ctx, button, path, body, { cue = null, staged = null, onOk = null, onFail = null } = {}) {
  const key = ctx.drawer.key;
  ctx.sound.play("confirm");
  button.disabled = true;
  let result = null;
  try {
    result = await ctx.session.act(path, body, {
      onOk,
      beforeRefresh: async (r) => {
        if (!r.ok) {
          ctx.sound.play("error");
          if (onFail) onFail(r);
          return;
        }
        if (cue) ctx.sound.play(cue);
        if (staged && !ctx.reduced() && !document.hidden) {
          button.classList.add("sent");
          button.textContent = t(staged);
          await wait(STAGED_MS);
        }
      },
    });
  } finally {
    button.disabled = false;
  }
  // the drawer may have been closed, or stepped to another application, while the request was out
  if (ctx.drawer.key === key) await ctx.drawer.reload();
  return result;
}
