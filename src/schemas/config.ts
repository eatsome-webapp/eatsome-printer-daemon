/**
 * Zod Schemas for Printer Daemon Configuration
 *
 * Validates configuration data exchanged between Tauri backend and React frontend.
 * Matches Rust structs in src-tauri/src/config.rs
 */

import { z } from 'zod'

/**
 * Connection Type Enum
 */
export const ConnectionTypeSchema = z.enum(['usb', 'network', 'bluetooth'])
export type ConnectionType = z.infer<typeof ConnectionTypeSchema>

/**
 * Printer Capabilities
 */
export const PrinterCapabilitiesSchema = z.object({
  cutter: z.boolean(),
  drawer: z.boolean(),
  qrcode: z.boolean(),
  max_width: z.number().int().positive(),
})
export type PrinterCapabilities = z.infer<typeof PrinterCapabilitiesSchema>

/**
 * Printer Configuration
 */
export const PrinterConfigSchema = z.object({
  id: z.string(),
  name: z.string(),
  connection_type: ConnectionTypeSchema,
  address: z.string(),
  protocol: z.string(),
  station: z.string().nullable(),
  is_primary: z.boolean(),
  capabilities: PrinterCapabilitiesSchema,
})
export type PrinterConfig = z.infer<typeof PrinterConfigSchema>

/**
 * App Configuration
 */
export const AppConfigSchema = z.object({
  version: z.string(),
  restaurant_id: z.string().nullable(),
  location_id: z.string().nullable(),
  auth_token: z.string().nullable(),
  supabase_url: z.string().url(),
  supabase_anon_key: z.string(),
  service_role_key: z.string(),
  printers: z.array(PrinterConfigSchema),
})
export type AppConfig = z.infer<typeof AppConfigSchema>

/**
 * Validate and parse config from Tauri backend
 *
 * @param data - Raw data from Tauri IPC
 * @returns Validated AppConfig
 * @throws ZodError if validation fails
 */
export function parseAppConfig(data: unknown): AppConfig {
  return AppConfigSchema.parse(data)
}

/**
 * Safely parse config with error handling
 *
 * @param data - Raw data from Tauri IPC
 * @returns Validated config or null if invalid
 */
export function safeParseAppConfig(data: unknown): AppConfig | null {
  const result = AppConfigSchema.safeParse(data)
  return result.success ? result.data : null
}

/**
 * Default empty configuration
 */
export const DEFAULT_CONFIG: AppConfig = {
  version: '1.0.0',
  restaurant_id: null,
  location_id: null,
  auth_token: null,
  supabase_url: 'https://gtlpzikuozrdgomsvqmo.supabase.co',
  supabase_anon_key: '',
  service_role_key: '',
  printers: [],
}
