import type { FC } from "hono/jsx";
import { getLampStyle } from "../lib/lamp";

interface LampCellProps {
  songId: number;
  title: string;
  difficulty: string;
  lamp: string;
  attributes: string | null;
}

export const LampCell: FC<LampCellProps> = ({
  songId,
  title,
  difficulty,
  lamp,
  attributes,
}) => {
  const style = getLampStyle(lamp);
  const lookupKey = `${songId}:${difficulty}`;

  const cellStyle: Record<string, string> = {
    padding: "4px 8px",
    "border-radius": "3px",
    "font-size": "0.8rem",
    "white-space": "nowrap",
    overflow: "hidden",
    "text-overflow": "ellipsis",
    "min-width": "0",
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
};
