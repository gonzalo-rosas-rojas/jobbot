// Acceptance test of the encargo docs/encargos/glosario-compuerta-en-seco.md (B2, in part: the words of the task). No text on screen
// says "compuerta" or "en seco" (in English "gate" or "dry run"): the last layer is the final review (revisión final, final check) and a
// form filled without sending it is a practice one (de prueba, practice run). The internal id `compuerta` stays (LAYERS, data-layer),
// and the palette's search aliases keep the old words on purpose, so that whoever learnt them finds the same.
// The image the practice run leaves is a capture (captura, capture), one word for it in every place (encargo
// docs/encargos/lote-pregunta-toast-check-captura.md): COLA, HOY and PROCESOS already said capture while the drawer said screenshot.
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { setLang, t } from "../src/i18n/index.js";

const OLD = { es: /\bcompuerta\b|\ben seco\b/i, en: /\bgate\b|\bdry[- ]?run\b/i };
const NEW = {
  es: { layer: "Revisión final", practice: /\bprueba\b/i, capture: /\bcaptura\b/i },
  en: { layer: "Final check", practice: /\bpractice[- ]run\b/i, capture: /\bcapture\b/i },
};
const NOT_CAPTURE = { es: /\bpantallazos?\b|\bcapturas? de pantalla\b/i, en: /\bscreenshots?\b/i };
const SEARCH_ALIASES = /^palette\.(alias|does)\./;

/** Every dictionary file of a language, merged: a string moved to another file is still read. */
async function dictionary(lang) {
  const dir = new URL(`../src/i18n/${lang}/`, import.meta.url);
  const files = readdirSync(dir).filter((name) => name.endsWith(".js"));
  return Object.assign({}, ...(await Promise.all(files.map(async (name) => (await import(new URL(name, dir))).default))));
}

for (const lang of ["es", "en"]) {
  test(`glosario (${lang}): no text on screen uses the old words of the last layer or of the practice run`, async () => {
    const entries = Object.entries(await dictionary(lang)).filter(([key]) => !SEARCH_ALIASES.test(key));
    assert.ok(entries.length > 500, "every dictionary is read");
    assert.deepEqual(entries.filter(([, text]) => OLD[lang].test(text)), []);
  });

  test(`glosario (${lang}): the last layer is called ${NEW[lang].layer} and the capture is of a practice run`, () => {
    setLang(lang);
    assert.equal(t("layer.compuerta"), NEW[lang].layer);
    assert.match(t("drawer.shot.alt"), NEW[lang].practice);
    assert.match(t("drawer.shot.alt"), NEW[lang].capture, "the drawer calls it what COLA and PROCESOS call it");
  });

  test(`glosario (${lang}): the image of the practice run is called a capture on every screen, never a screenshot`, async () => {
    const entries = Object.entries(await dictionary(lang)).filter(([key]) => !SEARCH_ALIASES.test(key));
    assert.deepEqual(entries.filter(([, text]) => NOT_CAPTURE[lang].test(text)), []);
  });
}

// Two strings the tests above do not name (reviewer, 2026-11-05): COLA's reason and PROCESOS's legend would pass the negative check with
// the old word simply deleted. Pinned here: the reason says the form was a practice one, and the list of layers ends in the final review.
const PINNED = {
  es: { why: /\bde prueba\b/i, last: "revisión final" },
  en: { why: /\bpractice run\b/i, last: "final check" },
};

for (const lang of ["es", "en"]) {
  test(`glosario (${lang}): the reason in COLA says practice, and the legend of PROCESOS ends its layers in ${PINNED[lang].last}`, () => {
    setLang(lang);
    assert.match(t("cola.why.base"), PINNED[lang].why);
    for (const key of ["procesos.legend.short", "procesos.legend.piles"]) {
      const layers = t(key).split(" (")[0];   // the long legend explains its solid and hollow piles after the list, in brackets
      assert.ok(layers.endsWith(PINNED[lang].last), `${key}: "${layers}" ends in ${PINNED[lang].last}`);
    }
  });
}
