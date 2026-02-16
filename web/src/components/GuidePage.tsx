import type { FC } from "hono/jsx";
import { Layout } from "./Layout";

interface GuidePageProps {
  user?: { username: string | null } | null;
}

export const GuidePage: FC<GuidePageProps> = ({ user }) => {
  const codeStyle =
    "background:#222;padding:8px 12px;border-radius:6px;display:block;font-family:monospace;font-size:0.85rem;color:#e0e0e0;overflow-x:auto;";

  return (
    <Layout title="Guide" user={user}>
      <div style="max-width:700px;margin:24px auto;">
        <h2 style="margin-bottom:24px;">Guide</h2>

        {/* Table of Contents */}
        <div class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">Contents</h3>
          <ul style="list-style:none;display:flex;flex-direction:column;gap:6px;">
            <li>
              <a href="#prerequisites">1. Prerequisites</a>
            </li>
            <li>
              <a href="#account-setup">2. Account Setup</a>
            </li>
            <li>
              <a href="#cli-setup">3. CLI Setup</a>
            </li>
            <li>
              <a href="#cli-login">4. CLI Login</a>
            </li>
            <li>
              <a href="#real-time-tracking">5. Real-time Tracking</a>
            </li>
            <li>
              <a href="#manual-upload">6. Manual Upload</a>
            </li>
            <li>
              <a href="#data-export">7. Data Export</a>
            </li>
            <li>
              <a href="#web-features">8. Web Features</a>
            </li>
          </ul>
        </div>

        {/* Prerequisites */}
        <div id="prerequisites" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">1. Prerequisites</h3>
          <ul style="list-style:disc;padding-left:20px;display:flex;flex-direction:column;gap:6px;color:#ccc;">
            <li>Windows PC</li>
            <li>
              beatmania IIDX INFINITAS installed and running
            </li>
            <li>An infst account</li>
          </ul>
        </div>

        {/* Account Setup */}
        <div id="account-setup" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">2. Account Setup</h3>
          <p style="color:#ccc;line-height:1.6;">
            Go to the{" "}
            <a href="/register">registration page</a> and create your account
            with an email, username, and password.
          </p>
        </div>

        {/* CLI Setup */}
        <div id="cli-setup" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">3. CLI Setup</h3>
          <p style="color:#ccc;line-height:1.6;margin-bottom:12px;">
            Download <code>infst.exe</code> from the latest{" "}
            <a
              href="https://github.com/dqn/infst/releases"
              target="_blank"
              rel="noopener noreferrer"
            >
              GitHub Release
            </a>
            . Place it in a directory of your choice.
          </p>
        </div>

        {/* CLI Login */}
        <div id="cli-login" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">4. CLI Login</h3>
          <p style="color:#ccc;line-height:1.6;margin-bottom:12px;">
            Run the following command to authenticate your CLI:
          </p>
          <code style={codeStyle}>infst login</code>
          <p style="color:#ccc;line-height:1.6;margin-top:12px;">
            A device code will be displayed. Open the verification URL in your
            browser, log in to your infst account, and enter the code to
            authorize the CLI. You can override the endpoint with{" "}
            <code>--endpoint &lt;URL&gt;</code>.
          </p>
        </div>

        {/* Real-time Tracking */}
        <div id="real-time-tracking" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">
            5. Real-time Tracking
          </h3>
          <p style="color:#ccc;line-height:1.6;margin-bottom:12px;">
            Start INFINITAS, then simply run:
          </p>
          <code style={codeStyle}>infst</code>
          <p style="color:#ccc;line-height:1.6;margin-top:12px;">
            The CLI automatically detects the game process, reads your play data
            in real time, and uploads results to the server after each song.
          </p>
        </div>

        {/* Manual Upload */}
        <div id="manual-upload" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">6. Manual Upload</h3>
          <p style="color:#ccc;line-height:1.6;margin-bottom:12px;">
            You can also upload previously exported data manually:
          </p>
          <code style={codeStyle}>
            infst upload --tracker tracker.tsv --mapping title-mapping.json
          </code>
        </div>

        {/* Data Export */}
        <div id="data-export" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">7. Data Export</h3>
          <p style="color:#ccc;line-height:1.6;margin-bottom:12px;">
            Export all your play data (scores, lamps, miss counts, DJ points)
            from the game:
          </p>
          <code style={codeStyle + "margin-bottom:8px;"}>
            infst export -o scores.tsv
          </code>
          <p style="color:#999;font-size:0.85rem;margin-top:8px;">
            Use <code>-f json</code> for JSON format.
          </p>
        </div>

        {/* Web Features */}
        <div id="web-features" class="card" style="margin-bottom:16px;">
          <h3 style="font-size:1rem;margin-bottom:12px;">8. Web Features</h3>
          <ul style="list-style:disc;padding-left:20px;display:flex;flex-direction:column;gap:6px;color:#ccc;">
            <li>View your clear lamps on difficulty tables</li>
            <li>Real-time updates as you play</li>
            <li>
              Public/private profile toggle in{" "}
              <a href="/settings">Settings</a>
            </li>
          </ul>
        </div>
      </div>
    </Layout>
  );
};
