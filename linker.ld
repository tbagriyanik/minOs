ENTRY(_start)

PHDRS {
    text PT_LOAD;
    rodata PT_LOAD;
    data PT_LOAD;
}

SECTIONS {
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);

    .data : {
        . = ALIGN(8);
        KEEP(*(.requests_start_marker))
        . = ALIGN(8);
        KEEP(*(.requests_base_revision))
        . = ALIGN(8);
        KEEP(*(.requests))
        . = ALIGN(8);
        KEEP(*(.requests_end_marker))
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
