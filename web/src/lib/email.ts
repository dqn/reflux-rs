import { Resend } from "resend";

export async function sendMagicLinkEmail(
  to: string,
  magicLinkUrl: string,
  apiKey: string,
): Promise<void> {
  const resend = new Resend(apiKey);

  await resend.emails.send({
    from: "infst <noreply@infst.dev>",
    to,
    subject: "Login to infst",
    html: `
      <div style="font-family: sans-serif; max-width: 480px; margin: 0 auto; padding: 24px;">
        <h2 style="color: #e0e0e0; background: #1a1a2e; padding: 16px; border-radius: 8px; text-align: center;">
          infst
        </h2>
        <p>Click the link below to log in. This link expires in 15 minutes.</p>
        <a href="${magicLinkUrl}" style="display: inline-block; padding: 12px 24px; background: #00e5ff; color: #000; text-decoration: none; border-radius: 4px; font-weight: bold;">
          Log in to infst
        </a>
        <p style="color: #888; font-size: 12px; margin-top: 24px;">
          If you did not request this, you can safely ignore this email.
        </p>
      </div>
    `,
  });
}
