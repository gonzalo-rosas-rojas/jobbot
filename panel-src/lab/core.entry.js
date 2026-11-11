// Lab for core/dom.js: exposes the builders to test/dom.test.mjs, which drives them in a real Chrome and checks that
// data stays text, that the refused attributes are refused and that the CSP is never violated.
import { h, sv, fill, link, twoStep, focusMark, restoreFocus, isTypingIn, readLaunch } from "../src/core/dom.js";

window.__dom = { h, sv, fill, link, twoStep, focusMark, restoreFocus, isTypingIn, readLaunch };
document.getElementById("out").append(h("p", { class: "ready" }, "ready"));
