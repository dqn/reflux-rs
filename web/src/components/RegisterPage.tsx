import { Layout } from "./Layout";

interface RegisterPageProps {
  error?: string;
}

export function RegisterPage({ error }: RegisterPageProps): ReturnType<typeof RegisterPage> {
  return (
    <Layout title="Register">
      <div style="max-width:400px;margin:48px auto;">
        <h2 style="margin-bottom:24px;">Choose a username</h2>
        <p style="color:#aaa;margin-bottom:16px;">
          Your username will be used in your profile URL.
        </p>
        {error ? <p class="error">{error}</p> : null}
        <form method="post" action="/auth/register">
          <div style="margin-bottom:12px;">
            <input
              type="text"
              name="username"
              placeholder="username"
              required
              pattern="[a-z0-9_\-]{3,20}"
              style="width:100%;"
            />
            <p style="font-size:0.8rem;color:#888;margin-top:4px;">
              3-20 characters. Lowercase letters, numbers, hyphens, underscores.
            </p>
          </div>
          <button type="submit" style="width:100%;">Register</button>
        </form>
      </div>
    </Layout>
  );
}
