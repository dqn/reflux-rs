import type { Child } from "hono/jsx";

interface LayoutProps {
  title?: string;
  user?: { username: string | null } | null;
  children: Child;
}

export function Layout({ title, user, children }: LayoutProps): ReturnType<typeof Layout> {
  const pageTitle = title ? `${title} - infst` : "infst";

  return (
    <html lang="ja">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{pageTitle}</title>
        <style>{`
          *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
          body {
            font-family: 'Segoe UI', system-ui, sans-serif;
            background: #0a0a1a;
            color: #e0e0e0;
            min-height: 100vh;
          }
          a { color: #00e5ff; text-decoration: none; }
          a:hover { text-decoration: underline; }
          nav {
            background: #12122a;
            border-bottom: 1px solid #2a2a4a;
            padding: 12px 24px;
            display: flex;
            align-items: center;
            justify-content: space-between;
          }
          nav .brand { font-weight: bold; font-size: 1.2rem; color: #00e5ff; }
          nav .links { display: flex; gap: 16px; align-items: center; }
          .container { max-width: 960px; margin: 0 auto; padding: 24px; }
          input, button, select {
            font-family: inherit;
            font-size: 0.9rem;
          }
          input[type="text"], input[type="email"] {
            background: #1a1a3a;
            border: 1px solid #3a3a5a;
            color: #e0e0e0;
            padding: 8px 12px;
            border-radius: 4px;
          }
          input:focus { outline: none; border-color: #00e5ff; }
          button {
            cursor: pointer;
            background: #00e5ff;
            color: #000;
            border: none;
            padding: 8px 16px;
            border-radius: 4px;
            font-weight: bold;
          }
          button:hover { background: #00c8e0; }
          button.secondary {
            background: #3a3a5a;
            color: #e0e0e0;
          }
          button.secondary:hover { background: #4a4a6a; }
          button.danger { background: #e53e3e; color: #fff; }
          button.danger:hover { background: #c53030; }
          .error { color: #e53e3e; margin-bottom: 12px; }
          .success { color: #48bb78; margin-bottom: 12px; }
        `}</style>
      </head>
      <body>
        <nav>
          <a class="brand" href="/">infst</a>
          <div class="links">
            {user ? (
              <>
                {user.username ? (
                  <a href={`/${user.username}`}>{user.username}</a>
                ) : null}
                <a href="/settings">Settings</a>
                <form method="post" action="/auth/logout" style="display:inline">
                  <button type="submit" class="secondary" style="padding:4px 12px;font-size:0.85rem">
                    Logout
                  </button>
                </form>
              </>
            ) : (
              <a href="/login">Login</a>
            )}
          </div>
        </nav>
        <div class="container">
          {children}
        </div>
      </body>
    </html>
  );
}
