import test from "node:test";
import assert from "node:assert/strict";
import { normalizeConfig, parseAudio, parseStatus, wavBytes, shouldStop } from "../overlay/tts-core.js";

// Cabecera WAV PCM mínima de 44 bytes más una muestra silenciosa.
const WAV = "UklGRiYAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YQIAAAAAAA==";
const audio = (extra = {}) => ({ id: "tts-1", mime: "audio/wav", wav_base64: WAV, text: "hola", user_name: "Ana", ...extra });
const status = (extra = {}) => ({ id: "tts-1", status: "playing", queue_len: 1, reason: null, message: null, ...extra });

test("normalizeConfig aplica defaults", () => {
  const defaults = { volume: 1, show_text: true, show_user: true };
  assert.deepEqual(normalizeConfig(), defaults);
  assert.deepEqual(normalizeConfig(null), defaults);
  assert.deepEqual(normalizeConfig([]), defaults);
});

test("normalizeConfig satura el volumen y exige booleanos", () => {
  assert.equal(normalizeConfig({ volume: 4 }).volume, 1);
  assert.equal(normalizeConfig({ volume: -1 }).volume, 0);
  assert.equal(normalizeConfig({ volume: 0.25 }).volume, 0.25);
  assert.equal(normalizeConfig({ volume: Number.NaN }).volume, 1);
  assert.equal(normalizeConfig({ volume: "0.5" }).volume, 1);
  assert.equal(normalizeConfig({ volume: Infinity }).volume, 1);
  const config = normalizeConfig({ show_text: false, show_user: "no" });
  assert.equal(config.show_text, false);
  assert.equal(config.show_user, true);
});

test("normalizeConfig no muta la entrada", () => {
  const raw = { volume: 3, show_text: false, extra: { nested: true } };
  const snapshot = structuredClone(raw);
  normalizeConfig(raw).volume = 0;
  assert.deepEqual(raw, snapshot);
});

test("parseAudio devuelve una copia normalizada", () => {
  const raw = audio({ extra: "ignorado" });
  const parsed = parseAudio(raw);
  assert.deepEqual(parsed, audio());
  assert.notEqual(parsed, raw);
  assert.equal(parseAudio(audio({ text: "<b>hola</b>" })).text, "<b>hola</b>");
});

test("parseAudio rechaza payloads rotos", () => {
  for (const broken of [
    null, "texto", [], audio({ id: "" }), audio({ id: " " }), audio({ id: 7 }),
    audio({ mime: "audio/mpeg" }), audio({ wav_base64: null }), audio({ text: 1 }), audio({ user_name: undefined }),
  ]) {
    assert.equal(parseAudio(broken), null, JSON.stringify(broken));
  }
});

test("parseStatus admite sólo los cuatro estados con shape seguro", () => {
  for (const kind of ["queued", "playing", "skipped", "error"]) {
    assert.deepEqual(parseStatus(status({ status: kind })), status({ status: kind }));
  }
  assert.deepEqual(parseStatus(status({ reason: "moderator_skip", message: "x" })).reason, "moderator_skip");
  assert.equal(parseStatus(status({ reason: undefined })).reason, null);
  for (const broken of [
    null, status({ status: "done" }), status({ status: "PLAYING" }), status({ id: "" }),
    status({ queue_len: -1 }), status({ queue_len: 1.5 }), status({ queue_len: "1" }),
    status({ reason: 3 }), status({ message: {} }),
  ]) {
    assert.equal(parseStatus(broken), null, JSON.stringify(broken));
  }
});

test("wavBytes decodifica un WAV mínimo", () => {
  const bytes = wavBytes(WAV);
  assert.ok(bytes instanceof Uint8Array);
  assert.equal(bytes.length, 46);
  assert.equal(String.fromCharCode(...bytes.subarray(0, 4)), "RIFF");
  assert.equal(String.fromCharCode(...bytes.subarray(8, 12)), "WAVE");
});

test("wavBytes rechaza base64 inválido y magic incorrecto", () => {
  for (const broken of [null, 12, "", "UklGR", "UklG*A==", "UklGRg===", btoa("RIFF\0\0\0\0WAVX"), btoa("RIFF")]) {
    assert.equal(wavBytes(broken), null, String(broken));
  }
});

test("wavBytes usa el decodificador inyectado y tolera su ausencia", () => {
  let calls = 0;
  const decode = (value) => {
    calls += 1;
    return atob(value);
  };
  assert.ok(wavBytes(WAV, decode));
  assert.equal(calls, 1);
  assert.equal(wavBytes(WAV, null), null);
  assert.equal(wavBytes(WAV, () => {
    throw new Error("roto");
  }), null);
});

test("shouldStop sólo detiene el id actual con skipped o error", () => {
  assert.equal(shouldStop("tts-1", status({ status: "skipped" })), true);
  assert.equal(shouldStop("tts-1", status({ status: "error" })), true);
  assert.equal(shouldStop("tts-1", status({ status: "playing" })), false);
  assert.equal(shouldStop("tts-1", status({ status: "queued" })), false);
  assert.equal(shouldStop("tts-2", status({ status: "skipped" })), false);
  assert.equal(shouldStop(null, status({ status: "skipped" })), false);
  assert.equal(shouldStop("", status({ id: "", status: "error" })), false);
  assert.equal(shouldStop("tts-1", null), false);
});
