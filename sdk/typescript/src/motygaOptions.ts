export type MotygaConfigValue = string | number | boolean | MotygaConfigValue[] | MotygaConfigObject;

export type MotygaConfigObject = { [key: string]: MotygaConfigValue };

export type MotygaOptions = {
  motygaPathOverride?: string;
  baseUrl?: string;
  apiKey?: string;
  /**
   * Additional `--config key=value` overrides to pass to the Motyga CLI.
   *
   * Provide a JSON object and the SDK will flatten it into dotted paths and
   * serialize values as TOML literals so they are compatible with the CLI's
   * `--config` parsing.
   */
  config?: MotygaConfigObject;
  /**
   * Environment variables passed to the Motyga CLI process. When provided, the SDK
   * will not inherit variables from `process.env`.
   */
  env?: Record<string, string>;
};
