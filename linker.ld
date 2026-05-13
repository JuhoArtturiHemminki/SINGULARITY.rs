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
        . = ALIGN(8);
        KEEP(*(.text.boot))
    } > SRAM_BOOT

    .text : {
        . = ALIGN(8);
        *(.vectors)
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > SRAM_TEXT

    .data : {
        . = ALIGN(8);
        _sdata = .;
        *(.data .data.*)
        _edata = .;
    } > SRAM_DATA

    .bss : {
        . = ALIGN(8);
        _sbss = .;
        *(.bss .bss.*)
        *(COMMON)
        _ebss = .;
    } > SRAM_DATA

    _stack_top = ALIGN(ORIGIN(SRAM_DATA) + LENGTH(SRAM_DATA) - 7, 16);
}
