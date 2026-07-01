let ctx: AudioContext | null = null;
let warmed = false;

function getContext(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

/** Pre-warm the AudioContext on a user gesture so it's ready when notifications fire. */
export function warmAudioContext(): void {
  if (warmed) return;
  const c = getContext();
  // Resume if suspended (browsers/webviews require a user gesture)
  if (c.state === "suspended") c.resume();
  // Play a silent buffer to fully initialize the audio graph
  const buf = c.createBuffer(1, 1, c.sampleRate);
  const src = c.createBufferSource();
  src.buffer = buf;
  src.connect(c.destination);
  src.start();
  warmed = true;
}

function playTone(freq: number, start: number, duration: number, gain = 0.15): void {
  const c = getContext();
  const osc = c.createOscillator();
  const vol = c.createGain();
  osc.type = "triangle";
  osc.frequency.value = freq;
  vol.gain.setValueAtTime(gain, start);
  vol.gain.exponentialRampToValueAtTime(0.001, start + duration);
  osc.connect(vol);
  vol.connect(c.destination);
  osc.start(start);
  osc.stop(start + duration);
}

export function playNeedsAttention(): void {
  const now = getContext().currentTime;
  playTone(880, now, 0.12);
  playTone(1100, now + 0.15, 0.12);
}

export function playTaskComplete(): void {
  const now = getContext().currentTime;
  playTone(523.25, now, 0.15, 0.12);
  playTone(659.25, now + 0.18, 0.15, 0.12);
  playTone(783.99, now + 0.36, 0.2, 0.12);
}
