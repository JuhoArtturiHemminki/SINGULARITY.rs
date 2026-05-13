MEMORY
{
    SRAM_BOOT (rx)  : ORIGIN = 0x40000000, LENGTH = 4K
    SRAM_TEXT (rx)  : ORIGIN = 0x40001000, LENGTH = 8K
    SRAM_JIT  (rwx) : ORIGIN = 0x40003000, LENGTH = 4K
    SRAM_DATA (rw)  : ORIGIN = 0x40004000, LENGTH = 16K
}

SECTIONS
{
    .boot : {
        KEEP(*(.text.boot))
    } > SRAM_BOOT

    .text : {
        *(.text .text.*)
        *(.vectors)
    } > SRAM_TEXT

    .data : {
        *(.data .data.*)
        *(.rodata .rodata.*)
        *(.bss .bss.*)
    } > SRAM_DATA

    _stack_top = ORIGIN(SRAM_DATA) + LENGTH(SRAM_DATA);
}
