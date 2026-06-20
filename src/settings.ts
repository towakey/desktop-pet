import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

interface AlarmConfig {
  enabled: boolean;
  hour: number;
  minute: number;
}

const enabledCheckbox = document.getElementById("alarm-enabled") as HTMLInputElement;
const enabledLabel = document.getElementById("alarm-enabled-label") as HTMLLabelElement;
const hourInput = document.getElementById("alarm-hour") as HTMLInputElement;
const minuteInput = document.getElementById("alarm-minute") as HTMLInputElement;
const saveBtn = document.getElementById("save-btn") as HTMLButtonElement;
const statusDiv = document.getElementById("status") as HTMLDivElement;
const autostartCheckbox = document.getElementById("autostart-enabled") as HTMLInputElement;
const autostartLabel = document.getElementById("autostart-enabled-label") as HTMLLabelElement;
const autostartStatus = document.getElementById("autostart-status") as HTMLDivElement;

function updateAlarmLabel() {
  enabledLabel.textContent = enabledCheckbox.checked ? "有効" : "無効";
}

function updateAutostartLabel() {
  autostartLabel.textContent = autostartCheckbox.checked ? "自動起動: 有効" : "自動起動: 無効";
}

async function loadAlarm() {
  try {
    const config = await invoke<AlarmConfig>("get_alarm");
    enabledCheckbox.checked = config.enabled;
    updateAlarmLabel();
    hourInput.value = String(config.hour);
    minuteInput.value = String(config.minute);
  } catch {
    statusDiv.textContent = "設定の読み込みに失敗しました";
  }
}

async function loadAutostart() {
  try {
    autostartCheckbox.checked = await isEnabled();
    updateAutostartLabel();
  } catch {
    autostartStatus.textContent = "自動起動状態の取得に失敗しました";
  }
}

enabledCheckbox.addEventListener("change", updateAlarmLabel);

autostartCheckbox.addEventListener("change", updateAutostartLabel);

saveBtn.addEventListener("click", async () => {
  const config: AlarmConfig = {
    enabled: enabledCheckbox.checked,
    hour: parseInt(hourInput.value, 10),
    minute: parseInt(minuteInput.value, 10),
  };

  if (isNaN(config.hour) || config.hour < 0 || config.hour > 23) {
    statusDiv.textContent = "時は0〜23で入力してください";
    return;
  }
  if (isNaN(config.minute) || config.minute < 0 || config.minute > 59) {
    statusDiv.textContent = "分は0〜59で入力してください";
    return;
  }

  try {
    await invoke("set_alarm", { config });
    statusDiv.textContent = config.enabled
      ? `保存しました: ${String(config.hour).padStart(2, "0")}:${String(config.minute).padStart(2, "0")}`
      : "アラームを無効にしました";
    setTimeout(() => { statusDiv.textContent = ""; }, 3000);
  } catch {
    statusDiv.textContent = "保存に失敗しました";
  }
});

autostartCheckbox.addEventListener("change", async () => {
  try {
    if (autostartCheckbox.checked) {
      await enable();
      autostartStatus.textContent = "Windows起動時に自動起動します";
    } else {
      await disable();
      autostartStatus.textContent = "自動起動を無効にしました";
    }
    setTimeout(() => { autostartStatus.textContent = ""; }, 3000);
  } catch {
    autostartCheckbox.checked = !autostartCheckbox.checked;
    updateAutostartLabel();
    autostartStatus.textContent = "自動起動設定の変更に失敗しました";
  }
});

loadAlarm();
loadAutostart();
