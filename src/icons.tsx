/** Ícones de traço simples, em SVG inline (sem biblioteca de ícones). */
const base = {
  width: 16, height: 16, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor",
  strokeWidth: 1.8, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true,
} as const;

export const IconMegaphone = () => (
  <svg {...base}>
    <path d="M3 10v4h4l6 4V6L7 10H3z" />
    <path d="M16.5 9a4 4 0 0 1 0 6" />
  </svg>
);

export const IconHeadphones = () => (
  <svg {...base}>
    <path d="M4 15v-3a8 8 0 0 1 16 0v3" />
    <rect x="3" y="14" width="4" height="6" rx="1.5" />
    <rect x="17" y="14" width="4" height="6" rx="1.5" />
  </svg>
);

export const IconDocument = () => (
  <svg {...base}>
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5z" />
    <path d="M14 3v5h5M9 13h6M9 17h6" />
  </svg>
);

export const IconFolder = () => (
  <svg {...base}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
  </svg>
);

export const IconPencil = () => (
  <svg {...base} width={12} height={12}>
    <path d="M4 20h4L19 9l-4-4L4 16v4z" />
    <path d="M13.5 6.5l4 4" />
  </svg>
);
