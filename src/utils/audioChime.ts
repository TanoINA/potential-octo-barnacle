/**
 * Desktop Notification and Audio Chime Utilities for AetherAgent.
 *
 * NOTE: As per design requirements, Audio Chime is MUTED BY DEFAULT (soundChimeEnabled: false).
 * Users can opt-in to sound effects via Settings -> Agent Defaults.
 */

// Web Audio API Synthesizer - No external audio file dependency
let audioCtx: AudioContext | null = null;

function getAudioContext(): AudioContext | null {
  if (typeof window === 'undefined') return null;
  const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  if (!AudioContextClass) return null;
  if (!audioCtx || audioCtx.state === 'closed') {
    audioCtx = new AudioContextClass();
  }
  return audioCtx;
}

/**
 * Plays a subtle, warm, modern chord (D5 587Hz -> A5 880Hz) to signal task completion.
 * Only called if user has enabled soundChimeEnabled in Settings.
 */
export async function playTaskCompleteChime(): Promise<void> {
  try {
    const ctx = getAudioContext();
    if (!ctx) return;
    if (ctx.state === 'suspended') {
      await ctx.resume();
    }

    const now = ctx.currentTime;
    const osc1 = ctx.createOscillator();
    const osc2 = ctx.createOscillator();
    const gain1 = ctx.createGain();
    const gain2 = ctx.createGain();

    // D5 tone (587.33 Hz)
    osc1.type = 'sine';
    osc1.frequency.setValueAtTime(587.33, now);

    // A5 tone (880.00 Hz)
    osc2.type = 'sine';
    osc2.frequency.setValueAtTime(880.0, now + 0.08);

    // Envelope for first note
    gain1.gain.setValueAtTime(0.001, now);
    gain1.gain.linearRampToValueAtTime(0.08, now + 0.04);
    gain1.gain.exponentialRampToValueAtTime(0.0001, now + 0.45);

    // Envelope for second note
    gain2.gain.setValueAtTime(0.0001, now);
    gain2.gain.setValueAtTime(0.001, now + 0.08);
    gain2.gain.linearRampToValueAtTime(0.07, now + 0.12);
    gain2.gain.exponentialRampToValueAtTime(0.0001, now + 0.55);

    osc1.connect(gain1);
    gain1.connect(ctx.destination);
    osc2.connect(gain2);
    gain2.connect(ctx.destination);

    osc1.start(now);
    osc1.stop(now + 0.46);
    osc2.start(now + 0.08);
    osc2.stop(now + 0.56);
  } catch (err) {
    console.warn('[AudioChime] Unable to play audio chime:', err);
  }
}

/**
 * Triggers a desktop notification using Web Notification API.
 */
export async function showDesktopNotification(title: string, body: string): Promise<void> {
  try {
    if (typeof window === 'undefined' || !('Notification' in window)) return;

    if (Notification.permission === 'granted') {
      new Notification(title, {
        body,
        icon: '/icons/128x128.png',
        silent: true, // Audio handled separately by user chime preference
      });
    } else if (Notification.permission !== 'denied') {
      const permission = await Notification.requestPermission();
      if (permission === 'granted') {
        new Notification(title, {
          body,
          icon: '/icons/128x128.png',
          silent: true,
        });
      }
    }
  } catch (err) {
    console.warn('[DesktopNotification] Failed to show notification:', err);
  }
}
