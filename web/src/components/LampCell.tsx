import { getLampStyle } from "../lib/lamp";

interface LampCellProps {
  title: string;
  infinitasTitle: string | null;
  difficulty: string;
  lamp: string;
  attributes: string | null;
}

export function LampCell({
  title,
  infinitasTitle,
  difficulty,
  lamp,
  attributes,
}: LampCellProps): ReturnType<typeof LampCell> {
  const style = getLampStyle(lamp);
  const lookupKey = `${infinitasTitle ?? title}:${difficulty}`;

  const cellStyle: Record<string, string> = {
    padding: "4px 8px",
    "border-radius": "3px",
    "font-size": "0.8rem",
    "white-space": "nowrap",
    overflow: "hidden",
    "text-overflow": "ellipsis",
    "max-width": "200px",
    display: "inline-block",
    color: style.color,
  };

  if (style.background.startsWith("linear-gradient")) {
    cellStyle["background-image"] = style.background;
  } else {
    cellStyle["background-color"] = style.background;
  }

  if (style.border) {
    cellStyle["border"] = style.border;
  }

  const styleStr = Object.entries(cellStyle)
    .map(([k, v]) => `${k}:${v}`)
    .join(";");

  return (
    <span
      class="lamp-cell"
      data-key={lookupKey}
      data-lamp={lamp}
      style={styleStr}
      title={attributes ? `${title} [${attributes}]` : title}
    >
      {title}
    </span>
  );
}
