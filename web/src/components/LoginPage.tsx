import { Layout } from "./Layout";

interface LoginPageProps {
  error?: string;
  success?: boolean;
}

export function LoginPage({ error, success }: LoginPageProps): ReturnType<typeof LoginPage> {
  return (
    <Layout title="Login">
      <div style="max-width:400px;margin:48px auto;">
        <h2 style="margin-bottom:24px;">Login</h2>
        {error ? <p class="error">{error}</p> : null}
        {success ? (
          <p class="success">Check your email for a login link.</p>
        ) : (
          <>
            <p style="color:#aaa;margin-bottom:16px;">
              Enter your email address to receive a login link.
            </p>
            <form id="login-form">
              <div style="margin-bottom:12px;">
                <input
                  type="email"
                  name="email"
                  placeholder="you@example.com"
                  required
                  style="width:100%;"
                />
              </div>
              <button type="submit" style="width:100%;">Send login link</button>
            </form>
            <script>{`
              document.getElementById('login-form').addEventListener('submit', function(e) {
                e.preventDefault();
                var email = this.querySelector('input[name="email"]').value;
                var btn = this.querySelector('button');
                btn.disabled = true;
                btn.textContent = 'Sending...';
                fetch('/auth/login', {
                  method: 'POST',
                  headers: { 'Content-Type': 'application/json' },
                  body: JSON.stringify({ email: email })
                }).then(function(res) {
                  if (res.ok) {
                    btn.textContent = 'Check your email!';
                    btn.style.background = '#48bb78';
                  } else {
                    btn.disabled = false;
                    btn.textContent = 'Send login link';
                    alert('Failed to send. Please try again.');
                  }
                }).catch(function() {
                  btn.disabled = false;
                  btn.textContent = 'Send login link';
                  alert('Network error. Please try again.');
                });
              });
            `}</script>
          </>
        )}
      </div>
    </Layout>
  );
}
