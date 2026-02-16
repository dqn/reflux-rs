import { raw } from "hono/html";
import type { Child, FC } from "hono/jsx";

interface LayoutProps {
  title?: string | undefined;
  user?: { username: string | null } | null | undefined;
  children: Child;
}

export const Layout: FC<LayoutProps> = ({ title, user, children }) => {
  const pageTitle = title ? `${title} - infst` : "infst";

  return (
    <html lang="ja">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <meta name="theme-color" content="#111111" />
        <meta name="description" content="IIDX INFINITAS Score Tracker" />
        <link rel="manifest" href="/manifest.webmanifest" />
        <link rel="apple-touch-icon" href="/icons/apple-touch-icon.png" />
        <meta name="apple-mobile-web-app-capable" content="yes" />
        <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
        <title>{pageTitle}</title>
        <style>{raw(`
          *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
          body {
            font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
            background: #111;
            color: #e0e0e0;
            min-height: 100vh;
          }
          a { color: #ccc; text-decoration: underline; }
          a:hover { color: #e0e0e0; }
          nav {
            background: #1a1a1a;
            border-bottom: 1px solid #2a2a2a;
            padding: 14px 24px;
          }
          nav .nav-inner {
            max-width: 960px;
            margin: 0 auto;
            display: flex;
            align-items: center;
            justify-content: space-between;
          }
          nav .brand {
            font-weight: bold;
            font-size: 1.2rem;
            color: #e0e0e0;
            text-decoration: none;
          }
          nav .brand:hover { color: #fff; }
          nav .links { display: flex; gap: 16px; align-items: center; }
          nav .links a {
            color: #999;
            text-decoration: none;
          }
          nav .links a:hover { color: #e0e0e0; }
          .container { max-width: 960px; margin: 0 auto; padding: 24px; }
          .card {
            background: #1a1a1a;
            border: 1px solid #2a2a2a;
            border-radius: 8px;
            padding: 24px;
          }
          input, button, select {
            font-family: inherit;
            font-size: 0.9rem;
          }
          input[type="text"], input[type="email"], input[type="password"] {
            background: #222;
            border: 1px solid #333;
            color: #e0e0e0;
            padding: 10px 12px;
            border-radius: 6px;
          }
          input:focus { outline: none; border-color: #888; }
          button {
            cursor: pointer;
            background: #e0e0e0;
            color: #111;
            border: none;
            padding: 10px 16px;
            border-radius: 6px;
            font-weight: bold;
          }
          button:hover { background: #ccc; }
          button.secondary {
            background: #333;
            color: #e0e0e0;
          }
          button.secondary:hover { background: #444; }
          button.danger { background: #c53030; color: #fff; }
          button.danger:hover { background: #a02020; }
          .error { color: #e06060; margin-bottom: 12px; }
          .success { color: #6bc98a; margin-bottom: 12px; }
        `)}</style>
      </head>
      <body>
        <nav>
          <div class="nav-inner">
            <a class="brand" href="/">infst</a>
            <div class="links">
              <a href="/guide">Guide</a>
              {user ? (
                <>
                  {user.username ? (
                    <a href={`/${user.username}`}>{user.username}</a>
                  ) : null}
                  <a href="/settings">Settings</a>
                  <form method="post" action="/auth/logout" style="display:inline">
                    <button type="submit" class="secondary" style="padding:6px 12px;font-size:0.85rem">
                      Logout
                    </button>
                  </form>
                </>
              ) : (
                <a href="/login">Login</a>
              )}
            </div>
          </div>
        </nav>
        <div class="container">
          {children}
        </div>
        {raw(`<script>
          if ("serviceWorker" in navigator) {
            navigator.serviceWorker.register("/sw.js");
          }
        </script>`)}
      </body>
    </html>
  );
};
