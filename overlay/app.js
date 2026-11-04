import { normalizeConfig, parseAudio, parseStatus, wavBytes, shouldStop } from "./tts-core.js";

const root = document.querySelector(".st-tts");
const user = root.querySelector(".st-tts__user");
const text = root.querySelector(".st-tts__text");
const statusLine = root.querySelector(".st-tts__status");
const audio = root.querySelector(".st-tts__audio");
const STATES = ["idle", "playing", "error"];
let config = normalizeConfig();
// Única reproducción activa: { id, audio, url, done }.
let current = null;

function setState(state) {
  root.classList.remove(...STATES.map((name) => `st-tts--${name}`));
  root.classList.add(`st-tts--${state}`);
}

async function sendDone(id, outcome) {
  try {
    await window.streams.emit("tts_done", { id, outcome });
  } catch (error) {
    console.warn("[tts] no se pudo confirmar la reproducción", error);
  }
}

function cleanup() {
  if (!current) return;
  current.audio.pause();
  current.audio.removeAttribute("src");
  current.audio.load();
  URL.revokeObjectURL(current.url);
  current = null;
}

async function finish(outcome) {
  if (!current || current.done) return;
  current.done = true;
  const { id } = current;
  cleanup();
  setState(outcome === "ended" ? "idle" : "error");
  await sendDone(id, outcome);
}

function play(data) {
  const item = parseAudio(data);
  if (!item) return;
  if (current) {
    void sendDone(item.id, "error");
    return;
  }
  const bytes = wavBytes(item.wav_base64);
  if (!bytes) {
    setState("error");
    void sendDone(item.id, "error");
    return;
  }
  const url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
  current = { id: item.id, audio, url, done: false };
  user.textContent = item.user_name;
  text.textContent = item.text;
  statusLine.textContent = "";
  audio.volume = config.volume;
  audio.src = url;
  setState("playing");
  audio.play().catch(() => finish("error"));
}

function showStatus(data) {
  const status = parseStatus(data);
  if (!status) return;
  statusLine.textContent = status.reason ? `${status.status}: ${status.reason}` : status.status;
  if (!shouldStop(current?.id, status)) return;
  // El worker ya avanzó: se limpia sin enviar un segundo tts_done.
  current.done = true;
  cleanup();
  setState(status.status === "error" ? "error" : "idle");
}

async function loadConfig() {
  try {
    const response = await fetch("./config.json");
    return response.ok ? await response.json() : {};
  } catch {
    return {};
  }
}

audio.addEventListener("ended", () => void finish("ended"));
audio.addEventListener("error", () => {
  if (current?.audio.getAttribute("src")) void finish("error");
});

config = normalizeConfig(await loadConfig());
user.hidden = !config.show_user;
text.hidden = !config.show_text;
window.streams.on("tts_audio", (data) => play(data));
window.streams.on("tts_status", (data) => showStatus(data));
