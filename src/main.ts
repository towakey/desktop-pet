import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

const canvas = document.getElementById("pet-canvas") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;
const alarmIndicator = document.getElementById("alarm-indicator")!;

let isAlarming = false;
let alarmAudioCtx: AudioContext | null = null;
let alarmOscillator: OscillatorNode | null = null;
let lastDismissedAlarmKey: string | null = null;

function getCurrentAlarmKey() {
  const now = new Date();
  return `${now.getHours()}:${now.getMinutes()}`;
}

function drawPolygonCharacter() {
  ctx.clearRect(0, 0, 160, 160);

  // Body - low-poly geometric shape (Porygon-inspired)
  ctx.beginPath();
  ctx.moveTo(80, 20);   // top
  ctx.lineTo(130, 50);  // top-right
  ctx.lineTo(145, 90);  // right
  ctx.lineTo(120, 130); // bottom-right
  ctx.lineTo(40, 130);  // bottom-left
  ctx.lineTo(15, 90);   // left
  ctx.lineTo(30, 50);   // top-left
  ctx.closePath();
  ctx.fillStyle = "#e8a0bf";
  ctx.fill();

  // Inner facet 1
  ctx.beginPath();
  ctx.moveTo(80, 20);
  ctx.lineTo(130, 50);
  ctx.lineTo(80, 75);
  ctx.closePath();
  ctx.fillStyle = "#f0b8d0";
  ctx.fill();

  // Inner facet 2
  ctx.beginPath();
  ctx.moveTo(80, 20);
  ctx.lineTo(30, 50);
  ctx.lineTo(80, 75);
  ctx.closePath();
  ctx.fillStyle = "#d890a8";
  ctx.fill();

  // Inner facet 3
  ctx.beginPath();
  ctx.moveTo(130, 50);
  ctx.lineTo(145, 90);
  ctx.lineTo(80, 75);
  ctx.closePath();
  ctx.fillStyle = "#d088a0";
  ctx.fill();

  // Inner facet 4
  ctx.beginPath();
  ctx.moveTo(15, 90);
  ctx.lineTo(30, 50);
  ctx.lineTo(80, 75);
  ctx.closePath();
  ctx.fillStyle = "#c880a0";
  ctx.fill();

  // Belly facet
  ctx.beginPath();
  ctx.moveTo(80, 75);
  ctx.lineTo(145, 90);
  ctx.lineTo(120, 130);
  ctx.lineTo(40, 130);
  ctx.lineTo(15, 90);
  ctx.closePath();
  ctx.fillStyle = "#f0c8d8";
  ctx.fill();

  // Eyes
  ctx.fillStyle = "#333";
  // Left eye
  ctx.beginPath();
  ctx.arc(60, 68, 6, 0, Math.PI * 2);
  ctx.fill();
  // Right eye
  ctx.beginPath();
  ctx.arc(100, 68, 6, 0, Math.PI * 2);
  ctx.fill();

  // Eye highlights
  ctx.fillStyle = "#fff";
  ctx.beginPath();
  ctx.arc(57, 65, 2.5, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(97, 65, 2.5, 0, Math.PI * 2);
  ctx.fill();

  // Tail (small triangle)
  ctx.beginPath();
  ctx.moveTo(145, 90);
  ctx.lineTo(155, 80);
  ctx.lineTo(150, 100);
  ctx.closePath();
  ctx.fillStyle = "#c070a0";
  ctx.fill();

  // Feet
  ctx.fillStyle = "#d090b0";
  // Left foot
  ctx.beginPath();
  ctx.moveTo(55, 130);
  ctx.lineTo(45, 145);
  ctx.lineTo(65, 145);
  ctx.closePath();
  ctx.fill();
  // Right foot
  ctx.beginPath();
  ctx.moveTo(105, 130);
  ctx.lineTo(95, 145);
  ctx.lineTo(115, 145);
  ctx.closePath();
  ctx.fill();
}

function startAlarmSound() {
  if (alarmAudioCtx) return;
  alarmAudioCtx = new AudioContext();
  alarmOscillator = alarmAudioCtx.createOscillator();
  const gainNode = alarmAudioCtx.createGain();

  alarmOscillator.type = "square";
  alarmOscillator.frequency.setValueAtTime(880, alarmAudioCtx.currentTime);
  gainNode.gain.setValueAtTime(0.15, alarmAudioCtx.currentTime);

  // Beep pattern
  const now = alarmAudioCtx.currentTime;
  for (let i = 0; i < 100; i++) {
    const t = now + i * 0.6;
    gainNode.gain.setValueAtTime(0.15, t);
    gainNode.gain.setValueAtTime(0, t + 0.3);
    alarmOscillator.frequency.setValueAtTime(880, t);
    alarmOscillator.frequency.setValueAtTime(660, t + 0.15);
  }

  alarmOscillator.connect(gainNode);
  gainNode.connect(alarmAudioCtx.destination);
  alarmOscillator.start();
}

function stopAlarmSound() {
  if (alarmOscillator) {
    alarmOscillator.stop();
    alarmOscillator = null;
  }
  if (alarmAudioCtx) {
    alarmAudioCtx.close();
    alarmAudioCtx = null;
  }
}

async function checkAlarm() {
  try {
    const shouldAlarm = await invoke<boolean>("check_alarm");
    const currentAlarmKey = getCurrentAlarmKey();
    if (lastDismissedAlarmKey !== currentAlarmKey) {
      lastDismissedAlarmKey = null;
    }
    if (shouldAlarm && !isAlarming && lastDismissedAlarmKey !== currentAlarmKey) {
      isAlarming = true;
      canvas.classList.add("alarm-flash");
      alarmIndicator.classList.add("active");
      startAlarmSound();
    }
  } catch {
    // ignore check errors
  }
}

// Click handler - open settings or stop alarm
const petContainer = document.getElementById("pet-container")!;
petContainer.addEventListener("click", async () => {
  if (isAlarming) {
    isAlarming = false;
    lastDismissedAlarmKey = getCurrentAlarmKey();
    canvas.classList.remove("alarm-flash");
    alarmIndicator.classList.remove("active");
    stopAlarmSound();
    return;
  }
  await invoke("open_settings");
});

// Enable dragging the character window
let isDragging = false;
petContainer.addEventListener("mousedown", (e) => {
  if (e.button === 0) {
    isDragging = true;
  }
});

document.addEventListener("mousemove", async () => {
  if (isDragging) {
    const win = getCurrentWindow();
    // Use startDragging for native window drag
    isDragging = false;
    await win.startDragging();
  }
});

document.addEventListener("mouseup", () => {
  isDragging = false;
});

// Initial draw
drawPolygonCharacter();

// Check alarm every 10 seconds
setInterval(checkAlarm, 10000);
