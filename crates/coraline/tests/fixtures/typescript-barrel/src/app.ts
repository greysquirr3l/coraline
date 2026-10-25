/**
 * A caller that imports the re-exported symbol.
 */

import { multiply } from './math';

export function run(x: number, y: number): number {
    return multiply(x, y);
}
