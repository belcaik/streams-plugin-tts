// Lógica pura del overlay TTS: sin DOM, Audio, timers ni red.

const STATUSES = ["queued", "playing", "skipped", "error"];
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const nonEmpty = (value) => typeof value === "string" && value.trim() !== "";
const optionalString = (value) => (value === null || value === undefined ? null : typeof value === "string" ? value : undefined);

export function normalizeConfig(raw = {}) {
  const source = isObject(raw) ? raw : {};
  const { volume } = source;
  return {
    volume: typeof volume === "number" && Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 1,
    show_text: typeof source.show_text === "boolean" ? source.show_text : true,
    show_user: typeof source.show_user === "boolean" ? source.show_user : true,
  };
}

export function parseAudio(data) {
  if (!isObject(data) || !nonEmpty(data.id) || data.mime !== "audio/wav") return null;
  if (typeof data.wav_base64 !== "string" || typeof data.text !== "string" || typeof data.user_name !== "string") {
    return null;
  }
  return { id: data.id, mime: data.mime, wav_base64: data.wav_base64, text: data.text, user_name: data.user_name };
}

export function parseStatus(data) {
  if (!isObject(data) || !nonEmpty(data.id) || !STATUSES.includes(data.status)) return null;
  const queueLen = data.queue_len;
  if (!Number.isInteger(queueLen) || queueLen < 0) return null;
  const reason = optionalString(data.reason);
  const message = optionalString(data.message);
  if (reason === undefined || message === undefined) return null;
  return { id: data.id, status: data.status, queue_len: queueLen, reason, message };
}

export function wavBytes(base64, decode = globalThis.atob) {
  if (typeof base64 !== "string" || base64.length % 4 !== 0 || !BASE64.test(base64)) return null;
  if (typeof decode !== "function") return null;
  let binary;
  try {
    binary = decode(base64);
  } catch {
    return null;
  }
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  const tag = (start) => String.fromCharCode(...bytes.subarray(start, start + 4));
  if (bytes.length < 12 || tag(0) !== "RIFF" || tag(8) !== "WAVE") return null;
  return bytes;
}

export function shouldStop(currentId, status) {
  return nonEmpty(currentId) && isObject(status) && status.id === currentId
    && (status.status === "skipped" || status.status === "error");
}
