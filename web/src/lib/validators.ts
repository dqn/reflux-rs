import { isValidLamp } from "./lamp";
import {
  PASSWORD_MIN_LENGTH,
  PASSWORD_MAX_LENGTH,
  USERNAME_PATTERN,
  RESERVED_USERNAMES,
} from "./constants";

interface ValidationResult {
  valid: boolean;
  error?: string;
}

export function validateLoginInput(email: unknown, password: unknown): ValidationResult {
  if (typeof email !== "string" || typeof password !== "string") {
    return { valid: false, error: "Email and password are required" };
  }
  return { valid: true };
}

export function validateRegisterInput(
  email: unknown,
  password: unknown,
  username: unknown,
): ValidationResult {
  if (
    typeof email !== "string" ||
    typeof password !== "string" ||
    typeof username !== "string"
  ) {
    return { valid: false, error: "All fields are required" };
  }

  if (password.length < PASSWORD_MIN_LENGTH || password.length > PASSWORD_MAX_LENGTH) {
    return {
      valid: false,
      error: `Password must be ${PASSWORD_MIN_LENGTH}-${PASSWORD_MAX_LENGTH} characters`,
    };
  }

  const trimmed = username.trim().toLowerCase();
  if (!USERNAME_PATTERN.test(trimmed)) {
    return {
      valid: false,
      error: "Username must be 3-20 characters (a-z, 0-9, -, _)",
    };
  }

  if (RESERVED_USERNAMES.includes(trimmed)) {
    return { valid: false, error: "This username is not available" };
  }

  return { valid: true };
}

interface LampInput {
  infinitasTitle?: unknown;
  difficulty?: unknown;
  lamp?: unknown;
  exScore?: unknown;
  missCount?: unknown;
}

export function validateLampInput(entry: LampInput): ValidationResult {
  if (
    typeof entry.infinitasTitle !== "string" ||
    typeof entry.difficulty !== "string" ||
    typeof entry.lamp !== "string"
  ) {
    return {
      valid: false,
      error: "infinitasTitle, difficulty, and lamp are required",
    };
  }

  if (!isValidLamp(entry.lamp)) {
    return { valid: false, error: "Invalid lamp value" };
  }

  return { valid: true };
}
