import test from "node:test";
import assert from "node:assert/strict";
import { setLang } from "../src/i18n/index.js";
import { factorPoints, factorText } from "../src/core/format.js";

test("factorPoints: a value that is not a number is empty text, never a made-up zero; a real zero stays", () => {
  setLang("es");
  for (const bad of [null, undefined, "", "  ", "abc", NaN, Infinity, true, {}, []]) assert.equal(factorPoints(bad), "", String(bad));
  assert.equal(factorPoints(0), "0");
  assert.equal(factorPoints("0"), "0");
  assert.equal(factorPoints(27), "27");
  assert.equal(factorPoints("27"), "27");
  assert.equal(factorPoints(12.34), "12,3");
  assert.equal(factorText("fit", null), null, "a bar with no points has no text");
});
