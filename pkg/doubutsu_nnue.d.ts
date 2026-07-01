/* tslint:disable */
/* eslint-disable */

export class AnimalShogiWasm {
    free(): void;
    [Symbol.dispose](): void;
    apply_human_drop(kind_val: number, to_sq: number): boolean;
    apply_human_move(from_sq: number, to_sq: number): boolean;
    get_board_string(): string;
    get_turn(): number;
    get_winner(): number;
    constructor();
    reset(): void;
    search_and_apply_move(time_limit_ms: bigint): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_animalshogiwasm_free: (a: number, b: number) => void;
    readonly animalshogiwasm_apply_human_drop: (a: number, b: number, c: number) => number;
    readonly animalshogiwasm_apply_human_move: (a: number, b: number, c: number) => number;
    readonly animalshogiwasm_get_board_string: (a: number) => [number, number];
    readonly animalshogiwasm_get_turn: (a: number) => number;
    readonly animalshogiwasm_get_winner: (a: number) => number;
    readonly animalshogiwasm_new: () => number;
    readonly animalshogiwasm_reset: (a: number) => void;
    readonly animalshogiwasm_search_and_apply_move: (a: number, b: bigint) => [number, number];
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
