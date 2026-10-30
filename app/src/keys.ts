// Maps browser key/mouse events to Windows virtual-key codes for push-to-talk,
// and back to readable labels.

const named: Record<string, [number, string]> = {
  Space: [0x20, "Boşluk"],
  Tab: [0x09, "Tab"],
  CapsLock: [0x14, "Caps Lock"],
  ShiftLeft: [0xa0, "Sol Shift"],
  ShiftRight: [0xa1, "Sağ Shift"],
  ControlLeft: [0xa2, "Sol Ctrl"],
  ControlRight: [0xa3, "Sağ Ctrl"],
  AltLeft: [0xa4, "Sol Alt"],
  AltRight: [0xa5, "Sağ Alt"],
  Backquote: [0xc0, "\""],
  Insert: [0x2d, "Insert"],
  Delete: [0x2e, "Delete"],
  Home: [0x24, "Home"],
  End: [0x23, "End"],
  PageUp: [0x21, "Page Up"],
  PageDown: [0x22, "Page Down"],
  ArrowUp: [0x26, "Yukarı ok"],
  ArrowDown: [0x28, "Aşağı ok"],
  ArrowLeft: [0x25, "Sol ok"],
  ArrowRight: [0x27, "Sağ ok"],
  Pause: [0x13, "Pause"],
  ScrollLock: [0x91, "Scroll Lock"],
};

export function vkFromKeyboard(e: KeyboardEvent): number | null {
  const code = e.code;
  if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
  if (/^Digit\d$/.test(code)) return 0x30 + Number(code[5]);
  if (/^Numpad\d$/.test(code)) return 0x60 + Number(code[6]);
  const f = /^F(\d{1,2})$/.exec(code);
  if (f && Number(f[1]) <= 24) return 0x6f + Number(f[1]);
  return named[code]?.[0] ?? null;
}

/** Mouse side buttons (back/forward) are the classic push-to-talk choice. */
export function vkFromMouse(e: MouseEvent): number | null {
  if (e.button === 1) return 0x04; // middle
  if (e.button === 3) return 0x05; // back
  if (e.button === 4) return 0x06; // forward
  return null;
}

export function vkLabel(vk: number | null): string {
  if (vk == null) return "Seçilmedi";
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk);
  if (vk >= 0x60 && vk <= 0x69) return `Numpad ${vk - 0x60}`;
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  if (vk === 0x04) return "Fare orta tuş";
  if (vk === 0x05) return "Fare geri tuşu";
  if (vk === 0x06) return "Fare ileri tuşu";
  return Object.values(named).find(([code]) => code === vk)?.[1] ?? `Tuş ${vk}`;
}
