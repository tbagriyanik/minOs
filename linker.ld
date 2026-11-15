ENTRY(_start)

PHDRS {
    text PT_LOAD;
    rodata PT_LOAD;
    data PT_LOAD;
    dynamic PT_DYNAMIC;
}

SECTIONS {
    /* KASLR: cekirdek ET_DYN (PIE) olarak baglanir; Limine bu tabani
       rastgele bir yuksek-yari adresine kaydirir (bkz. boot/limine.conf `kaslr`). */
    . = 0;

    .text : {
        *(.text .text.*)
        *(.plt .plt.got .plt.sec .iplt)
    } :text

    . = ALIGN(4096);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    /* PIE/KASLR metadata must remain separate output sections.  Placing
       these into .rodata makes lld assign them SHT_PROGBITS, while their
       input types are SHT_RELA/SHT_DYNSYM/SHT_STRTAB/etc. */
    .rela.dyn : { *(.rela.dyn) *(.rela.plt) *(.rela*) } :rodata
    .relr.dyn : { *(.relr.dyn) } :rodata
    .dynsym : { *(.dynsym) } :rodata
    .dynstr : { *(.dynstr) } :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .hash : { *(.hash) } :rodata
    .gnu.version : { *(.gnu.version) } :rodata
    .gnu.version_d : { *(.gnu.version_d) } :rodata
    .gnu.version_r : { *(.gnu.version_r) } :rodata

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
        *(.got .got.*)
    } :data

    /* Limine ET_DYN çekirdekleri için PT_DYNAMIC segmenti.
       Bu segmentin dosya içeriği .dynamic bölümünü içerir. */
    .dynamic : {
        *(.dynamic)
    } :dynamic

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
