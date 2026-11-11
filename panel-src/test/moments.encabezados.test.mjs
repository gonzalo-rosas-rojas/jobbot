// Acceptance test of the encargo docs/encargos/encabezado-con-pregunta.md (B1: each stop says what it is for). The header of HOY,
// COLA, APRENDER and SISTEMA says the stop's question and the hour ("¿Qué me toca hoy? 09:00") instead of the name of the story
// ("El relevo, 09:00"). The questions are the ones of the palette (stop.<id>.ask). PROCESOS and the prologue keep theirs.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { sistemaView } from "../src/moments/sistema/view.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const VIEWS = { hoy: hoyView, cola: colaView, aprender: aprenderView, sistema: sistemaView };
const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const STORY = /relevo|compuerta|escritorio de noche|sala de máquinas|handover|\bgate\b|night desk|engine room/i;

for (const lang of ["es", "en"]) {
  test(`encabezados (${lang}): HOY, COLA, APRENDER and SISTEMA open with their question and the hour`, () => {
    setLang(lang);
    setTimeZone("America/Santiago");
    for (const name of ["rich", "empty"]) {
      const snap = fixture(name);
      for (const [id, view] of Object.entries(VIEWS)) {
        const { eyebrow } = view(model(snap), { nowMs: Date.parse(snap.generated_at) });
        assert.match(eyebrow, new RegExp(`^${escape(t(`stop.${id}.ask`))} \\d{2}:\\d{2}$`), `${name}/${id}`);
        assert.doesNotMatch(eyebrow, STORY, `${name}/${id}: the name of the story is gone`);
      }
    }
  });
}
