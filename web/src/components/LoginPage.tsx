import { Layout } from "./Layout";

interface LoginPageProps {
  error?: string;
  values?: { email?: string };
}

export function LoginPage({ error, values }: LoginPageProps): ReturnType<typeof LoginPage> {
  return (
    <Layout title="Login">
      <div style="max-width:400px;margin:48px auto;">
        <h2 style="margin-bottom:24px;">Login</h2>
        {error ? <p class="error">{error}</p> : null}
        <form method="post" action="/auth/login">
          <div style="margin-bottom:12px;">
            <input
              type="email"
              name="email"
              placeholder="you@example.com"
              required
              value={values?.email ?? ""}
              style="width:100%;"
            />
          </div>
          <div style="margin-bottom:12px;">
            <input
              type="password"
              name="password"
              placeholder="Password"
              required
              minLength={8}
              maxLength={72}
              style="width:100%;"
            />
          </div>
          <button type="submit" style="width:100%;">Login</button>
        </form>
        <p style="margin-top:16px;text-align:center;color:#aaa;">
          Don't have an account? <a href="/register">Register</a>
        </p>
      </div>
    </Layout>
  );
}
