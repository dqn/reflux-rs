// Session
export const SESSION_MAX_AGE_SECONDS = 604800; // 7 days

// Device code flow
export const DEVICE_CODE_EXPIRY_MS = 300_000; // 5 minutes
export const POLLING_INTERVAL_SECONDS = 5;

// Password constraints
export const PASSWORD_MIN_LENGTH = 8;
export const PASSWORD_MAX_LENGTH = 72;

// Username constraints
export const USERNAME_MIN_LENGTH = 3;
export const USERNAME_MAX_LENGTH = 20;
export const USERNAME_PATTERN = /^[a-z0-9_-]{3,20}$/;

// Reserved usernames
export const RESERVED_USERNAMES = [
  "login",
  "register",
  "settings",
  "auth",
  "api",
  "admin",
  "guide",
];

// Rate limiting
export const RATE_LIMIT_LOGIN_MAX = 5;
export const RATE_LIMIT_LOGIN_WINDOW_SECONDS = 60;
export const RATE_LIMIT_REGISTER_MAX = 3;
export const RATE_LIMIT_REGISTER_WINDOW_SECONDS = 3600;
export const RATE_LIMIT_DEVICE_CODE_MAX = 10;
export const RATE_LIMIT_DEVICE_CODE_WINDOW_SECONDS = 60;

// API token expiry
export const API_TOKEN_EXPIRY_DAYS = 90;

// Cleanup
export const CLEANUP_PROBABILITY = 0.1; // 10% chance per poll
