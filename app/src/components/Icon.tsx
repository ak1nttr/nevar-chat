// The handful of line icons the app needs, drawn on a 24px grid.

const paths = {
  mic: "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3Z M5 11a7 7 0 0 0 14 0 M12 18v3",
  micOff: "M9 9v3a3 3 0 0 0 5.1 2.1 M15 9.3V6a3 3 0 0 0-5.7-1.3 M5 11a7 7 0 0 0 11.6 5.3 M19 11a7 7 0 0 1-.6 2.8 M12 18v3 M3 3l18 18",
  headphones: "M4 15v-3a8 8 0 0 1 16 0v3 M4 15a2 2 0 0 1 2-2h1v7H6a2 2 0 0 1-2-2Z M20 15a2 2 0 0 0-2-2h-1v7h1a2 2 0 0 0 2-2Z",
  headphonesOff: "M4 15v-3a8 8 0 0 1 12.5-6.6 M20 12v3 M4 15a2 2 0 0 1 2-2h1v7H6a2 2 0 0 1-2-2Z M17 13h1a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-1Z M3 3l18 18",
  phone: "M5 4h3l2 5-2.5 1.5a11 11 0 0 0 6 6L15 14l5 2v3a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2Z",
  hangUp: "M3 14.5c5-4.7 13-4.7 18 0l-2.3 2.6-3.6-1.5v-2.4a10 10 0 0 0-6.2 0v2.4l-3.6 1.5Z",
  settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z",
  copy: "M9 9h10v10H9Z M5 15V5h10",
  plus: "M12 5v14 M5 12h14",
  close: "M6 6l12 12 M18 6L6 18",
  check: "M5 12.5l4.5 4.5L19 7",
  back: "M15 5l-7 7 7 7",
  pin: "M12 17v5 M9 4h6 M10 4v6.5l-3.5 3.5V17h11v-3l-3.5-3.5V4",
  minimize: "M6 12h12",
  send: "M4 12h15 M13 6l6 6-6 6",
  message: "M4 5h16v11H9l-5 4Z",
  userPlus: "M3 20c0-3 2.7-5 6-5s6 2 6 5 M9 12a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7 M19 8v6 M16 11h6",
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
