import { Layout } from "./Layout";

interface RegisterPageProps {
  error?: string;
  values?: { email?: string; username?: string };
}

export function RegisterPage({ error, values }: RegisterPageProps): ReturnType<typeof RegisterPage> {
  return (
    <Layout title="Register">
      <div style="max-width:400px;margin:48px auto;">
        <h2 style="margin-bottom:24px;">Create an account</h2>
        {error ? <p class="error">{error}</p> : null}
        <form method="post" action="/auth/register">
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
              type="text"
              name="username"
              placeholder="username"
              required
              pattern="[a-z0-9_\-]{3,20}"
              value={values?.username ?? ""}
              style="width:100%;"
            />
            <p style="font-size:0.8rem;color:#888;margin-top:4px;">
              3-20 characters. Lowercase letters, numbers, hyphens, underscores.
            </p>
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
            <p style="font-size:0.8rem;color:#888;margin-top:4px;">
              8-72 characters.
            </p>
          </div>
          <button type="submit" style="width:100%;">Register</button>
        </form>
        <p style="margin-top:16px;text-align:center;color:#aaa;">
          Already have an account? <a href="/login">Login</a>
        </p>
      </div>
    </Layout>
  );
}
