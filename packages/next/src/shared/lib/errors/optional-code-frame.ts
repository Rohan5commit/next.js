// Optional code-frame rendering using native bindings
// This module gracefully degrades when bindings are unavailable either because they haven't
// loaded yet or because the native dependencies are excluded from the build
import type {
  NapiCodeFrameLocation,
  NapiCodeFrameOptions,
} from '../../../build/swc/generated-native'

import type { Binding } from '../../../build/swc/types'

let cached: (() => Binding | undefined) | undefined

function tryGetBindingsSync(): Binding | undefined {
  if (!cached) {
    try {
      // Use a dynamic failable require so that we can no-op this module in production
      cached = (
        require('../../../build/swc') as typeof import('../../../build/swc')
      ).tryGetBindingsSync
    } catch {
      cached = () => undefined
    }
  }
  return cached()
}

/**
 * Renders a code frame showing the location of an error in source code.
 * Returns `undefined` if native bindings are not available.
 *
 * This function is safe to use in `next start` production runtime where
 * native bindings may not be present. It will gracefully return `undefined`
 * if bindings cannot be loaded.
 *
 * @param file - The source code to render
 * @param location - The location to highlight (line and column numbers are 1-indexed)
 * @param options - Optional configuration
 * @returns The formatted code frame string, or undefined if bindings unavailable
 */
export function renderCodeFrameIfNativeBindingsAvailable(
  file: string,
  location: NapiCodeFrameLocation,
  options?: NapiCodeFrameOptions
): string | undefined {
  const bindings = tryGetBindingsSync()
  if (!bindings) {
    return undefined
  }
  return bindings.codeFrameColumns(file, location, defaultOptions(options))
}

export function defaultOptions(
  options: NapiCodeFrameOptions = {}
): NapiCodeFrameOptions {
  // default to the terminal width.
  if (options.maxWidth === undefined) {
    options.maxWidth = process.stdout.columns
  }
  return options
}

export type { NapiCodeFrameLocation, NapiCodeFrameOptions }
