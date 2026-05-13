#![no_std]
#![no_main]
#![allow(dead_code)]

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::panic::PanicInfo;

const MAX_CORES: usize = 4;
pub const NEUROMORPHIC_CHANNELS: usize = 256;
const SIMD_WORDS: usize = NEUROMORPHIC_CHANNELS / 32;

const SYS_STATUS_REG: *mut u32 = 0x4000_1000 as *mut u32;
const INTERRUPT_CLEAR_REG: *mut u32 = 0x4000_100C as *mut u32;
const TSENSOR_CORE_REG: *const u32 = 0x4000_10A0 as *const u32;
const DIAG_LOG_REG: *mut u32 = 0x4000_10B4 as *mut u32;
const SYS_SHUTDOWN_CTRL: *mut u32 = 0x4000_10C0 as *mut u32;

const EVOLUTION_EXEC_SEGMENT: *mut u8 = 0x4000_3000 as *mut u8;
const TRANSLATION_TABLE_BASE: *mut u64 = 0x4000_8000 as *mut u64;

static JITTER_ERR_COUNT: AtomicU32 = AtomicU32::new(0);
const MAX_ALLOWED_JITTER_STRIKES: u32 = 3;
const TSENSOR_TEMP_MASK: u32 = 0x0000_00FF;
const MAX_SAFE_TEMPERATURE: u32 = 85;
const HARDWARE_SHUTDOWN_MAGIC: u32 = 0xDEAD_BEEF;

pub enum SdprMode {
    ModeX = 500,
    ModeS = 300,
}

#[repr(C, align(64))]
pub struct CompressedNeuralMatrix {
    pub synaptic_weights: [i8; NEUROMORPHIC_CHANNELS],
    pub potentials: [u8; NEUROMORPHIC_CHANNELS],
    pub threshold: u8,
    pub leak_shift: u8,
    pub learning_rate: i8,
}

#[derive(Copy, Clone)]
pub struct TimingChromosome {
    pub target_ticks: u32,
    pub allowed_margin: u32,
    pub generation_id: u32,
}

pub struct SingularityProductionCore {
    pub pmu_rails: [*mut u32; 4],
    pub current_mode: SdprMode,
    pub is_active: AtomicBool,
    pub active_chromosome: TimingChromosome,
    pub neural_matrix: CompressedNeuralMatrix,
}

#[no_mangle]
#[link_section = ".text.boot"]
pub unsafe extern "C" fn _start() -> ! {
    #[cfg(target_arch = "aarch64")]
    {
        asm!(
            "msr DAIFSet, #0xf",
            "ldr x0, =_stack_top",
            "mov sp, x0",
            "ldr x1, =vector_table",
            "msr vbar_el1, x1",
            "mrs x2, sctlr_el1",
            "orr x2, x2, #0x1",
            "msr sctlr_el1, x2",
            "isb",
            "bl kernel_main",
            options(noreturn)
        );
    }
    #[cfg(target_arch = "x86_64")]
    {
        asm!(
            "cli",
            "mov rsp, offset _stack_top",
            "call kernel_main",
            options(noreturn)
        );
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        loop {}
    }
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main() -> ! {
    let rails = [
        0x4000_6000 as *mut u32,
        0x4000_6004 as *mut u32,
        0x4000_6008 as *mut u32,
        0x4000_600C as *mut u32,
    ];
    
    let mut core_engine = SingularityProductionCore {
        pmu_rails: rails,
        current_mode: SdprMode::ModeS,
        is_active: AtomicBool::new(false),
        active_chromosome: TimingChromosome {
            target_ticks: 42,
            allowed_margin: 5,
            generation_id: 0,
        },
        neural_matrix: CompressedNeuralMatrix {
            synaptic_weights: [0; NEUROMORPHIC_CHANNELS],
            potentials: [0; NEUROMORPHIC_CHANNELS],
            threshold: 10,
            leak_shift: 1,
            learning_rate: 2,
        },
    };

    core_engine.run_free_running_singularity_loop();
}

#[inline(always)]
fn get_cycle_count() -> u64 {
    let cycles: u64;
    unsafe {
        #[cfg(target_arch = "aarch64")]
        asm!("mrs {}, pmccntr_el0", out(reg) cycles, options(nomem, nostack));
        #[cfg(target_arch = "x86_64")]
        {
            let low: u32;
            let high: u32;
            asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack));
            cycles = ((high as u64) << 32) | (low as u64);
        }
        #[cfg(target_arch = "riscv64")]
        asm!("rdcycle {}", out(reg) cycles, options(nomem, nostack));
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64", target_arch = "riscv64")))]
        { cycles = 0; }
    }
    cycles
}

#[inline(always)]
pub unsafe fn read_core_temperature() -> u32 {
    core::ptr::read_volatile(TSENSOR_CORE_REG) & TSENSOR_TEMP_MASK
}

#[inline(always)]
pub unsafe fn read_historical_jitter() -> u32 {
    JITTER_ERR_COUNT.load(Ordering::Relaxed)
}

#[inline(always)]
pub unsafe fn log_hardware_diagnostic(event_code: u32) {
    core::ptr::write_volatile(DIAG_LOG_REG, event_code);
    #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));
}

#[inline(always)]
pub unsafe fn emergency_rail_severance(&self) -> ! {
    log_hardware_diagnostic(0xEEEE_FFFF);
    for rail in self.pmu_rails.iter() {
        core::ptr::write_volatile(*rail, 0);
    }
    core::ptr::write_volatile(SYS_SHUTDOWN_CTRL, HARDWARE_SHUTDOWN_MAGIC);
    #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));
    loop {
        #[cfg(target_arch = "x86_64")] asm!("hlt", options(noreturn));
        #[cfg(target_arch = "aarch64")] asm!("wfi", options(noreturn));
        #[cfg(target_arch = "riscv64")] asm!("wfi", options(noreturn));
    }
}

impl SingularityProductionCore {
    #[inline(always)]
    pub fn simulate_neural_step(&mut self, incoming_spikes: &[u32; SIMD_WORDS]) -> [u32; SIMD_WORDS] {
        let mut outgoing_spikes = [0u32; SIMD_WORDS];

        for w in 0..SIMD_WORDS {
            let mut spike_word = incoming_spikes[w];
            let mut out_word = 0u32;
            let base_idx = w * 32;

            while spike_word != 0 {
                let bit_idx = spike_word.trailing_zeros() as usize;
                let i = base_idx + bit_idx;

                let weight = self.neural_matrix.synaptic_weights[i];
                let current_pot = self.neural_matrix.potentials[i];
                
                let mut new_pot = current_pot.saturating_add_signed(weight);

                if new_pot >= self.neural_matrix.threshold {
                    out_word |= 1 << bit_idx;
                    new_pot = 0;
                    self.neural_matrix.synaptic_weights[i] = self.neural_matrix.synaptic_weights[i]
                        .saturating_add(self.neural_matrix.learning_rate);
                } else {
                    self.neural_matrix.synaptic_weights[i] = self.neural_matrix.synaptic_weights[i]
                        .saturating_sub(self.neural_matrix.learning_rate >> 1);
                }

                self.neural_matrix.potentials[i] = new_pot;
                spike_word &= spike_word - 1; 
            }

            for bit_idx in 0..32 {
                let i = base_idx + bit_idx;
                if (incoming_spikes[w] & (1 << bit_idx)) == 0 {
                    self.neural_matrix.potentials[i] = self.neural_matrix.potentials[i] >> self.neural_matrix.leak_shift;
                }
            }

            outgoing_spikes[w] = out_word;
        }

        outgoing_spikes
    }

    #[inline(always)]
    unsafe fn fire_resonant_pulse(&self, rail: *mut u32) {
        let target_ticks = self.active_chromosome.target_ticks;
        let start = get_cycle_count();

        core::ptr::write_volatile(rail, 0xFFFFFFFF);
        #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));

        for _ in 0..target_ticks {
            #[cfg(target_arch = "aarch64")] asm!("isb", options(nostack, nomem));
            #[cfg(target_arch = "x86_64")]  asm!("pause", options(nostack, nomem));
            #[cfg(target_arch = "riscv64")] asm!("fence", options(nostack, nomem));
            #[cfg(not(any(target_arch="aarch64", target_arch="x86_64", target_arch="riscv64")))] asm!("nop");
        }

        core::ptr::write_volatile(rail, 0x00000000);
        #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));

        if get_cycle_count() - start > (target_ticks + self.active_chromosome.allowed_margin) as u64 {
            JITTER_ERR_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    unsafe fn molecular_reset_apr(&self) {
        for rail in self.pmu_rails.iter() {
            core::ptr::write_volatile(*rail, 0x55555555);
        }
        #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));
    }

    pub unsafe fn mutate_translation_attributes(&self, spike_signature: u32) {
        let mut bytecode_buffer: [u8; 32] = [0; 32];
        let mut idx = 0;

        let attr_mask: u64 = if spike_signature > 0 { 0x0040_0000_0000_0705 } else { 0x0040_0000_0000_0707 };
        core::ptr::write_volatile(TRANSLATION_TABLE_BASE, attr_mask);
        #[cfg(target_arch = "aarch64")] asm!("dsb sy", options(nostack, nomem));

        let target_page = EVOLUTION_EXEC_SEGMENT as usize;

        #[cfg(target_arch = "aarch64")]
        {
            let val_low = (target_page & 0xFFFF) as u32;
            let movz_instr = 0xD2800000 | (val_low << 5) | 0; 
            bytecode_buffer[idx..idx+4].copy_from_slice(&movz_instr.to_le_bytes());
            idx += 4;

            asm!("tlbi vae1is, {}", in(reg) (target_page >> 12), options(nostack, nomem));
            asm!("dsb ish", options(nostack, nomem));
            asm!("isb", options(nostack, nomem));

            bytecode_buffer[idx..idx+4].copy_from_slice(&[0xC0, 0x03, 0x5F, 0xD6]);
            idx += 4;
        }

        #[cfg(target_arch = "x86_64")]
        {
            bytecode_buffer[idx] = 0x48; 
            bytecode_buffer[idx+1] = 0xB8; 
            let addr_bytes = target_page.to_le_bytes();
            bytecode_buffer[idx+2..idx+10].copy_from_slice(&addr_bytes);
            idx += 10;

            asm!("invlpg [{}]", in(reg) target_page, options(nostack, nomem));

            bytecode_buffer[idx] = 0xC3; 
            idx += 1;
        }

        core::ptr::copy_nonoverlapping(bytecode_buffer.as_ptr(), EVOLUTION_EXEC_SEGMENT, idx);
        self.flush_pipeline_instruction_cache(idx);
        log_hardware_diagnostic(0x000A_0000 | self.active_chromosome.generation_id);

        let evolved_subroutine: extern "C" fn() = core::mem::transmute::<*mut u8, extern "C" fn()>(EVOLUTION_EXEC_SEGMENT);
        evolved_subroutine();
    }

    unsafe fn flush_pipeline_instruction_cache(&self, len: usize) {
        #[cfg(target_arch = "aarch64")]
        {
            let mut ptr = EVOLUTION_EXEC_SEGMENT as usize;
            let end = ptr + len;
            while ptr < end {
                asm!("dc cvau, {}", in(reg) ptr, options(nostack, nomem));
                ptr += 64;
            }
            asm!("dsb ish", options(nostack, nomem));
            ptr = EVOLUTION_EXEC_SEGMENT as usize;
            while ptr < end {
                asm!("ic ivau, {}", in(reg) ptr, options(nostack, nomem));
                ptr += 64;
            }
            asm!("dsb ish", options(nostack, nomem));
            asm!("isb", options(nostack, nomem));
        }
        #[cfg(target_arch = "x86_64")]
        {
            let mut ptr = EVOLUTION_EXEC_SEGMENT as usize;
            let end = ptr + len;
            asm!("mfence", options(nostack, nomem));
            while ptr < end {
                asm!("clflush [{}]", in(reg) ptr, options(nostack, nomem));
                ptr += 64;
            }
            asm!("sfence", options(nostack, nomem));
        }
    }

    pub unsafe fn run_free_running_singularity_loop(&mut self) -> ! {
        let mut neural_input = [0u32; SIMD_WORDS];

        loop {
            let current_jitter = read_historical_jitter();
            let current_temp = read_core_temperature();

            if current_jitter >= MAX_ALLOWED_JITTER_STRIKES || current_temp > MAX_SAFE_TEMPERATURE {
                self.emergency_rail_severance();
            }

            if self.is_active.swap(true, Ordering::SeqCst) {
                continue;
            }

            for rail in self.pmu_rails.iter() {
                self.fire_resonant_pulse(*rail);
            }

            self.molecular_reset_apr();

            let cycle_seed = get_cycle_count() as u32;
            neural_input[0] = cycle_seed ^ current_jitter;
            neural_input[1] = current_temp;

            let outgoing_spikes = self.simulate_neural_step(&neural_input);
            
            let mut combined_signature = 0u32;
            for word in outgoing_spikes.iter() {
                combined_signature ^= (word >> 16) ^ (word & 0xFFFF);
            }

            self.mutate_translation_attributes(combined_signature);

            if current_jitter > 0 {
                let mut seed = cycle_seed ^ combined_signature;
                if seed == 0 { seed = 0xACE1; }
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;

                let delta = (seed & 0x01) + 1;
                let mut candidate = self.active_chromosome;

                if seed % 2 == 0 {
                    candidate.target_ticks = candidate.target_ticks.saturating_add(delta);
                } else {
                    candidate.target_ticks = candidate.target_ticks.saturating_sub(delta);
                }

                if candidate.target_ticks >= 30 && candidate.target_ticks <= 60 {
                    candidate.generation_id += 1;
                    candidate.allowed_margin = if candidate.target_ticks > 45 { 8 } else { 5 };
                    self.active_chromosome = candidate;
                }
            }

            self.is_active.store(false, Ordering::SeqCst);

            #[cfg(target_arch = "x86_64")] asm!("pause", options(nostack, nomem));
            #[cfg(target_arch = "aarch64")] asm!("yield", options(nostack, nomem));
            #[cfg(target_arch = "riscv64")] asm!("nop", options(nostack, nomem));
        }
    }
}

#[no_mangle]
#[link_section = ".vectors"]
pub static vector_table: [u32; 16] = [0; 16]; 

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        core::ptr::write_volatile(DIAG_LOG_REG, 0xBBBB_FFFF);
        core::ptr::write_volatile(SYS_SHUTDOWN_CTRL, HARDWARE_SHUTDOWN_MAGIC);
        loop {
            #[cfg(target_arch = "x86_64")] asm!("hlt", options(noreturn));
            #[cfg(target_arch = "aarch64")] asm!("wfi", options(noreturn));
            #[cfg(target_arch = "riscv64")] asm!("wfi", options(noreturn));
        }
    }
}
