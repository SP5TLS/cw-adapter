/* RP2350 linker fragment.
 *
 * The RP2350 BOOTROM scans the first 4 KiB of flash for an IMAGE_DEF block
 * to validate the image. embassy-rp injects a default `IMAGE_DEF` static
 * into the `.start_block` section when its `_rp235x` feature is enabled;
 * we place that section into the dedicated START_BLOCK region defined in
 * memory.x (0x10000000 .. 0x10000100). Reserving the region — rather than
 * inserting the section virtually after `.vector_table` — prevents `.text`
 * from overlapping the IMAGE_DEF.
 */

SECTIONS {
    .start_block ORIGIN(START_BLOCK) :
    {
        KEEP(*(.start_block));
    } > START_BLOCK
} INSERT BEFORE .vector_table;
