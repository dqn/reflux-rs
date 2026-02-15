import { LampCell } from "./LampCell";
import type { LampValue } from "../lib/lamp";
import { LAMP_VALUES, getLampStyle } from "../lib/lamp";

interface TableEntry {
  id: number;
  title: string;
  infinitasTitle: string | null;
  difficulty: string;
  attributes: string | null;
  lamp: string;
  exScore: number | null;
  missCount: number | null;
}

interface TierGroup {
  tier: string;
  entries: TableEntry[];
}

interface TableViewProps {
  tableKey: string;
  tiers: TierGroup[];
  username: string;
}

export function TableView({
  tableKey,
  tiers,
  username,
}: TableViewProps): ReturnType<typeof TableView> {
  // Calculate statistics
  const allEntries = tiers.flatMap((t) => t.entries);
  const lampCounts = new Map<string, number>();
  for (const entry of allEntries) {
    lampCounts.set(entry.lamp, (lampCounts.get(entry.lamp) ?? 0) + 1);
  }

  return (
    <div>
      <h2 style="margin-bottom: 16px;">{tableKey}</h2>

      {/* Statistics bar */}
      <div style="margin-bottom: 24px;">
        <div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:8px;">
          {LAMP_VALUES.filter((l) => (lampCounts.get(l) ?? 0) > 0).map((lamp) => {
            const style = getLampStyle(lamp);
            const count = lampCounts.get(lamp) ?? 0;
            return (
              <span
                style={`font-size:0.85rem;padding:2px 8px;border-radius:3px;color:${style.color};background:${style.background.startsWith("linear") ? style.background : style.background}${style.border ? `;border:${style.border}` : ""}`}
              >
                {lamp}: {count}
              </span>
            );
          })}
        </div>
        <div style="font-size:0.85rem;color:#888;">
          Total: {allEntries.length}
        </div>
      </div>

      {/* Tier groups */}
      {tiers.map((tier) => (
        <div style="margin-bottom: 20px;">
          <h3 style="font-size:1rem;color:#aaa;border-bottom:1px solid #2a2a4a;padding-bottom:4px;margin-bottom:8px;">
            {tier.tier}
          </h3>
          <div style="display:flex;flex-wrap:wrap;gap:4px;">
            {tier.entries.map((entry) => (
              <LampCell
                title={entry.title}
                infinitasTitle={entry.infinitasTitle}
                difficulty={entry.difficulty}
                lamp={entry.lamp}
                attributes={entry.attributes}
              />
            ))}
          </div>
        </div>
      ))}

      {/* Polling script */}
      <script>{`
        (function() {
          var username = ${JSON.stringify(username)};
          var lastPoll = new Date().toISOString();

          var LAMP_STYLES = ${JSON.stringify(
            Object.fromEntries(
              LAMP_VALUES.map((l) => [l, getLampStyle(l)])
            )
          )};

          function updateCell(key, lamp) {
            var cells = document.querySelectorAll('.lamp-cell[data-key="' + key + '"]');
            cells.forEach(function(cell) {
              var style = LAMP_STYLES[lamp] || LAMP_STYLES["NO PLAY"];
              cell.dataset.lamp = lamp;
              cell.style.color = style.color;
              if (style.background.indexOf("linear-gradient") === 0) {
                cell.style.backgroundImage = style.background;
                cell.style.backgroundColor = "";
              } else {
                cell.style.backgroundColor = style.background;
                cell.style.backgroundImage = "";
              }
              cell.style.border = style.border || "";
            });
          }

          function poll() {
            fetch("/api/lamps/updated-since?since=" + encodeURIComponent(lastPoll) + "&user=" + encodeURIComponent(username))
              .then(function(res) { return res.json(); })
              .then(function(data) {
                if (data.lamps && data.lamps.length > 0) {
                  data.lamps.forEach(function(l) {
                    updateCell(l.infinitasTitle + ":" + l.difficulty, l.lamp);
                  });
                  lastPoll = new Date().toISOString();
                }
              })
              .catch(function() {});
          }

          setInterval(poll, 5000);
        })();
      `}</script>
    </div>
  );
}
