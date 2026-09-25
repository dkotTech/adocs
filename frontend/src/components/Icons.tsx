interface IconProps {
  size?: number;
  class?: string;
}

function svg(props: IconProps, children: preact.ComponentChildren) {
  const s = props.size ?? 16;
  return (
    <svg
      class={props.class}
      width={s}
      height={s}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const ChevronRight = (p: IconProps) => svg(p, <path d="m9 18 6-6-6-6" />);
export const Search = (p: IconProps) => svg(p, <><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></>);
export const PanelLeft = (p: IconProps) => svg(p, <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></>);
export const Expand = (p: IconProps) => svg(p, <><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></>);
export const Shrink = (p: IconProps) => svg(p, <><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></>);
export const ExternalLink = (p: IconProps) => svg(p, <><path d="M15 3h6v6M10 14 21 3M21 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5" /></>);
export const Folder = (p: IconProps) => svg(p, <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />);
export const File = (p: IconProps) => svg(p, <><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></>);
export const Download = (p: IconProps) => svg(p, <><path d="M12 3v12M6 11l6 6 6-6M4 21h16" /></>);
export const Printer = (p: IconProps) => svg(p, <><path d="M6 9V3h12v6" /><rect x="3" y="9" width="18" height="8" rx="2" /><path d="M7 14h10v7H7z" /></>);
export const Close = (p: IconProps) => svg(p, <path d="M18 6 6 18M6 6l12 12" />);
export const Refresh = (p: IconProps) => svg(p, <><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5M3 21v-5h5" /></>);
