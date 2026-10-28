MEMORY
{
  /* FLASH and RAM are the regions cortex-m-rt / link.x expects */

  /* FLASH (flash): Full FLASH range, for reference only (dual bank, BANK_2 starts at 0x8040000) */
  /* FLASH : ORIGIN = 0x08000000, LENGTH = 512K */

  /* FLASH (flash): Application */
  FLASH : ORIGIN = 0x08000000, LENGTH = 504K

  /* SETTINGS (flash): ww_settings store, 4 erase pages (the end of BANK_2: erases don't stall code running from bank 1) */
  SETTINGS : ORIGIN = 0x0807E000, LENGTH = 8K

  /* SRAM (ram): Main RAM (also exported as RAM) */
  /* SRAM : ORIGIN = 0x20000000, LENGTH = 144K */
  RAM : ORIGIN = 0x20000000, LENGTH = 144K

  /* BKPSRAM (reg): TAMP_BKP0R..3R, retained across resets (not power loss without VBAT); cnt.x places the BKP counters
     here. TAMP_BKP4R (0x4000B110) is the boot word of bedrock_rom_boot (src/boot.rs), not cnt's */
  BKPSRAM : ORIGIN = 0x4000B100, LENGTH = 0x10
}

/* Helper symbols: additional RAM banks (see init_ram.rs), flash partitions */
/* SETTINGS: offsets relative to the start of flash (ORIGIN(FLASH)), as embassy-boot / flash drivers expect */
__settings_start = ORIGIN(SETTINGS) - ORIGIN(FLASH);
__settings_end = ORIGIN(SETTINGS) + LENGTH(SETTINGS) - ORIGIN(FLASH);

/* GNU build id (--build-id=sha1 in build.rs): the firmware's identity, a host finds the ELF by it. Kept in flash;
   ww_firmware_info::build_id() reads it through the two symbols. */
SECTIONS
{
  .note.gnu.build-id : ALIGN(4)
  {
    __build_id_start = .;
    KEEP(*(.note.gnu.build-id))
    __build_id_end = .;
  } > FLASH
} INSERT AFTER .rodata;

/* ES0548 2.2.10 (STM32G0B1/G0C1 errata, every silicon revision): a code prefetch can fail on a branch or call into
   the other flash bank and run wrong instructions. Flash prefetch is on (src/main.rs), so all code stays in bank 1.
   When this fails: turn prefetch off in src/main.rs and delete this ASSERT, or make the code smaller (opt-level "s"). */
ASSERT(__etext <= 0x08040000, "code reaches flash bank 2 with prefetch on (ES0548 2.2.10): see memory.x");
