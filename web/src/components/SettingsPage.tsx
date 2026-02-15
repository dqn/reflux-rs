import { Layout } from "./Layout";

interface SettingsPageProps {
  user: {
    username: string | null;
    apiToken: string | null;
    isPublic: boolean;
  };
}

export function SettingsPage({ user }: SettingsPageProps): ReturnType<typeof SettingsPage> {
  return (
    <Layout title="Settings" user={user}>
      <div style="max-width:500px;margin:24px auto;">
        <h2 style="margin-bottom:24px;">Settings</h2>

        {/* API Token */}
        <section style="margin-bottom:32px;">
          <h3 style="margin-bottom:12px;font-size:1rem;">API Token</h3>
          <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px;">
            <input
              type="text"
              id="api-token"
              value={user.apiToken ?? ""}
              readonly
              style="flex:1;font-family:monospace;font-size:0.85rem;"
            />
            <button type="button" id="copy-token" style="white-space:nowrap;">
              Copy
            </button>
          </div>
          <button type="button" id="regen-token" class="danger" style="font-size:0.85rem;">
            Regenerate
          </button>
        </section>

        {/* Visibility */}
        <section style="margin-bottom:32px;">
          <h3 style="margin-bottom:12px;font-size:1rem;">Profile Visibility</h3>
          <label style="display:flex;align-items:center;gap:8px;cursor:pointer;">
            <input
              type="checkbox"
              id="is-public"
              checked={user.isPublic}
            />
            <span>Public profile</span>
          </label>
          <p style="font-size:0.8rem;color:#888;margin-top:4px;">
            When disabled, your lamp data will not be visible to others.
          </p>
        </section>

        <script>{`
          // Copy token
          document.getElementById('copy-token').addEventListener('click', function() {
            var input = document.getElementById('api-token');
            navigator.clipboard.writeText(input.value).then(function() {
              var btn = document.getElementById('copy-token');
              btn.textContent = 'Copied!';
              setTimeout(function() { btn.textContent = 'Copy'; }, 2000);
            });
          });

          // Regenerate token
          document.getElementById('regen-token').addEventListener('click', function() {
            if (!confirm('Are you sure? Existing API clients will need the new token.')) return;
            fetch('/api/users/me/token/regenerate', { method: 'POST' })
              .then(function(res) { return res.json(); })
              .then(function(data) {
                if (data.apiToken) {
                  document.getElementById('api-token').value = data.apiToken;
                }
              });
          });

          // Toggle visibility
          document.getElementById('is-public').addEventListener('change', function() {
            fetch('/api/users/me', {
              method: 'PATCH',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ isPublic: this.checked })
            });
          });
        `}</script>
      </div>
    </Layout>
  );
}
