export type MainAppIconName =
  | "panel"
  | "plus"
  | "today"
  | "review"
  | "write"
  | "memory"
  | "settings"
  | "more"
  | "arrow"
  | "send"
  | "clock"
  | "book"
  | "chat"
  | "minimize"
  | "maximize"
  | "restore"
  | "close"
  | "search"
  | "chevron"
  | "back";

type MainAppIconProps = {
  name: MainAppIconName;
};

function IconPaths({ name }: MainAppIconProps) {
  switch (name) {
    case "panel":
      return <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></>;
    case "plus":
      return <path d="M12 5v14M5 12h14" />;
    case "today":
      return <><rect x="4" y="5" width="16" height="15" rx="2" /><path d="M8 3v4M16 3v4M4 9h16M8 13h3v3H8z" /></>;
    case "review":
      return <><path d="M4 12a8 8 0 1 0 2.34-5.66L4 8.68" /><path d="M4 4v4.68h4.68M12 8v4l2.5 1.5" /></>;
    case "write":
      return <><path d="M4 20h4l11-11a2.8 2.8 0 0 0-4-4L4 16v4z" /><path d="M13.5 6.5l4 4" /></>;
    case "memory":
      return <><path d="M8 4h8a3 3 0 0 1 3 3v10a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3V7a3 3 0 0 1 3-3z" /><path d="M9 2v4M15 2v4M9 10h6M9 14h6" /></>;
    case "settings":
      return <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.82 2.82-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1v.1h-4v-.1A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.82-2.82.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1-.4h-.1v-4H3A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.82-2.82.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1v-.1h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.82 2.82-.06.06A1.7 1.7 0 0 0 19.4 9c.15.36.36.7.6 1 .26.28.62.42 1 .4h.1v4H21a1.7 1.7 0 0 0-1.6 1z" /></>;
    case "more":
      return <><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>;
    case "arrow":
      return <path d="M5 12h14M13 6l6 6-6 6" />;
    case "send":
      return <path d="M21 3 10 14m11-11-7 18-4-7-7-4 18-7Z" />;
    case "clock":
      return <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>;
    case "book":
      return <><path d="M5 4h10a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3V4z" /><path d="M8 16h10M8 8h6" /></>;
    case "chat":
      return <path d="M20 15a3 3 0 0 1-3 3H9l-5 3V7a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v8z" />;
    case "minimize":
      return <path d="M4 12h16" />;
    case "maximize":
      return <rect x="5" y="5" width="14" height="14" rx="1" />;
    case "restore":
      return <><path d="M8 8V5h11v11h-3" /><rect x="5" y="8" width="11" height="11" rx="1" /></>;
    case "close":
      return <path d="m6 6 12 12M18 6 6 18" />;
    case "search":
      return <><circle cx="11" cy="11" r="6.5" /><path d="m16 16 4 4" /></>;
    case "chevron":
      return <path d="m7 10 5 5 5-5" />;
    case "back":
      return <path d="M19 12H5M11 18l-6-6 6-6" />;
  }
}

function MainAppIcon({ name }: MainAppIconProps) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <IconPaths name={name} />
    </svg>
  );
}

export default MainAppIcon;
