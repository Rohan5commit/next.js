import { getBindingsSync } from '../../../build/swc'
import type {
  NapiCodeFrameLocation,
  NapiCodeFrameOptions,
} from '../../../build/swc/generated-native'

import { defaultOptions } from './optional-code-frame'

/**
 * Renders a code frame showing the location of an error in source code
 *
 * Performs best effort syntax highlighting using ANSI codes and ensures that
 * error locations are centered in the frame by horizontally scrolling if
 * necessary.
 *
 * Uses the native Rust implementation for:
 * - Better performance on large files
 * - Proper handling of long lines
 * - Memory efficiency
 * - Best-effort syntax highlighting using a regex tokenizer
 *
 * @param file - The source code to render
 * @param location - The location to highlight (line and column numbers are 1-indexed)
 * @param options - Optional configuration
 * @returns The formatted code frame string
 * @throws if the native bindings have not been installed
 */
export function renderCodeFrame(
  file: string,
  location: NapiCodeFrameLocation,
  options?: NapiCodeFrameOptions
): string {
  return getBindingsSync().codeFrameColumns(
    file,
    location,
    defaultOptions(options)
  )
}
