use crate::{
    sched::{Attributes, CpuSet, Pid, Policy, SchedFlags},
    unix::error::{ret_into_result, ret_to_result},
};
use std::ffi::c_int;

#[cfg(target_os = "linux")]
use syscalls::{syscall, Errno, Sysno};

#[cfg(target_os = "linux")]
#[allow(non_camel_case_types)]
pub type pid_t = std::ffi::c_int;

pub const SCHED_IDLE: u32 = 5;
pub const SCHED_NORMAL: u32 = 0;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_DEADLINE: u32 = 6;
pub const SCHED_EXT: u32 = 7;

// #[cfg(target_os = "macos")]
// fn sched2qos(sched_policy: u32) -> u32 {
//     use libc::qos_class_t;

//     match sched_policy {
//         SCHED_IDLE => qos_class_t::QOS_CLASS_BACKGROUND,
//         SCHED_NORMAL => 0,
//         SCHED_BATCH => 0,
//         SCHED_FIFO => 0,
//         SCHED_RR => 0,
//         SCHED_DEADLINE => 0,
//         SCHED_EXT => 0,
//     }
// }

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
#[repr(C)]
pub struct SchedAttr {
    /// Size of this structure
    pub size: u32, /* Size of this structure */
    /// Policy (SCHED_*)
    pub sched_policy: u32,
    /// Flags
    pub sched_flags: u64,
    /// Nice value (SCHED_OTHER, SCHED_BATCH)
    pub sched_nice: i32,

    /// Static priority (SCHED_FIFO, SCHED_RR)
    pub sched_priority: u32,

    /// For SCHED_DEADLINE
    pub sched_runtime: u64,
    /// For SCHED_DEADLINE
    pub sched_deadline: u64,
    /// For SCHED_DEADLINE
    pub sched_period: u64,

    /// Utilization hints
    pub sched_util_min: u32,
    /// Utilization hints
    pub sched_util_max: u32,
}
/// The sched_setattr() system call sets the scheduling policy and
/// associated attributes for the thread whose ID is specified in
/// `pid`. If `pid` equals zero, the scheduling policy and attributes of
/// the calling thread will be set.
#[allow(clippy::missing_safety_doc)]
#[cfg(target_os = "linux")]
pub unsafe fn sched_set_attr(pid: pid_t, attr: *mut SchedAttr, flags: u32) -> Result<usize, Errno> {
    syscall!(Sysno::sched_setattr, pid, attr, flags)
}
#[allow(clippy::missing_safety_doc)]
#[cfg(target_os = "linux")]
pub unsafe fn sched_get_attr(
    pid: pid_t,
    attr: *mut SchedAttr,
    size: u32,
    flags: u32,
) -> Result<usize, Errno> {
    syscall!(Sysno::sched_getattr, pid, attr, size, flags)
}

/// The [get_attr()] function wraps the `sched_getattr()` system call and fetches the scheduling policy and
/// the associated attributes for the thread whose ID is specified in pid.
pub fn get_attr(pid: Pid) -> Result<Attributes, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        let mut attr = SchedAttr {
            size: 0,
            sched_policy: 0,
            sched_flags: 0,
            sched_nice: 0,
            sched_priority: 0,
            sched_runtime: 0,
            sched_deadline: 0,
            sched_period: 0,
            sched_util_min: 0,
            sched_util_max: 0,
        };

        let ret = unsafe {
            sched_get_attr(
                pid.as_raw(),
                &mut attr,
                core::mem::size_of::<SchedAttr>() as u32,
                0,
            )
        };
        match ret {
            Ok(_) => {
                let a = Attributes {
                    policy: Policy::from_raw(attr.sched_policy).unwrap(),
                    flags: SchedFlags::from_bits_truncate(attr.sched_flags as i16),
                    nice: attr.sched_nice,
                    priority: attr.sched_priority,
                    deadline_ns: attr.sched_deadline,
                    period_ns: attr.sched_period,
                    runtime_ns: attr.sched_runtime,
                    sched_util_min: attr.sched_util_min,
                    sched_util_max: attr.sched_util_max,
                };
                Ok(a)
            }
            Err(_) => Err(std::io::Error::last_os_error()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        use libc::{
            thread_extended_policy_data_t, thread_policy_flavor_t, thread_policy_t, thread_t,
            thread_time_constraint_policy_data_t, THREAD_EXTENDED_POLICY,
            THREAD_EXTENDED_POLICY_COUNT, THREAD_TIME_CONSTRAINT_POLICY,
            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        };

        use crate::unix::macos::mach_to_result;

        // macOS has no per-thread POSIX nice values; see [set_thread_nice()].
        let nice = get_thread_nice(pid)?;

        // Real-time policies map to THREAD_TIME_CONSTRAINT_POLICY. macOS
        // cannot distinguish Fifo, RoundRobin, and Deadline from one
        // another; policies of this group report as Fifo. An explicit
        // time-constraint policy always wins over the extended policy,
        // because [set_attr()] re-enables the timeshare extended policy
        // whenever it leaves the real-time group.
        let mut tc = thread_time_constraint_policy_data_t {
            period: 0,
            computation: 0,
            constraint: 0,
            preemptible: 0,
        };
        let mut count = THREAD_TIME_CONSTRAINT_POLICY_COUNT;
        let mut get_default = 0;

        let ret = unsafe {
            libc::thread_policy_get(
                pid.as_raw() as thread_t,
                THREAD_TIME_CONSTRAINT_POLICY as thread_policy_flavor_t,
                &mut tc as *mut _ as thread_policy_t,
                &mut count,
                &mut get_default,
            )
        };

        if mach_to_result(ret, ()).is_ok() && get_default == 0 {
            return Ok(Attributes {
                policy: Policy::Fifo,
                flags: SchedFlags::empty(),
                nice,
                priority: 0,
                runtime_ns: u64::from(tc.computation),
                deadline_ns: u64::from(tc.constraint),
                period_ns: u64::from(tc.period),
                sched_util_min: 0,
                sched_util_max: 0,
            });
        }

        // The Idle policy is stored in the precedence importance; see
        // [DARWIN_IDLE_IMPORTANCE]. The Idle policy is only applicable
        // without a nice value.
        if nice == -DARWIN_IDLE_IMPORTANCE {
            return Ok(Attributes {
                policy: Policy::Idle,
                flags: SchedFlags::empty(),
                nice: 0,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            });
        }

        // Normal versus Batch: [set_attr()] maps Normal to the timeshare
        // (timeshare = true) extended policy and Batch to the non-timeshare
        // (timeshare = false) extended policy.
        let mut ext = thread_extended_policy_data_t { timeshare: 0 };
        let mut count = THREAD_EXTENDED_POLICY_COUNT;
        let mut get_default = 0;

        let ret = unsafe {
            libc::thread_policy_get(
                pid.as_raw() as thread_t,
                THREAD_EXTENDED_POLICY as thread_policy_flavor_t,
                &mut ext as *mut _ as thread_policy_t,
                &mut count,
                &mut get_default,
            )
        };

        mach_to_result(
            ret,
            Attributes {
                policy: if ext.timeshare == 0 {
                    Policy::Batch
                } else {
                    Policy::Normal
                },
                flags: SchedFlags::empty(),
                nice,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            },
        )
    }
}

/// Stores the nice value as the precedence policy's importance on macOS.
/// macOS has no per-thread POSIX nice values; the closest thread-scoped
/// equivalent is the precedence policy's importance, which is added to the
/// thread's base priority. A positive nice value lowers
/// the scheduling priority, so it is stored as a negative importance.
#[cfg(target_os = "macos")]
fn set_thread_nice(pid: Pid, nice: i32) -> Result<(), std::io::Error> {
    use crate::unix::macos::mach_to_result;
    use libc::{
        thread_policy_flavor_t, thread_precedence_policy_data_t, thread_t,
        THREAD_PRECEDENCE_POLICY, THREAD_PRECEDENCE_POLICY_COUNT,
    };

    let mut prec = thread_precedence_policy_data_t { importance: -nice };
    let ret = unsafe {
        libc::thread_policy_set(
            pid.as_raw() as thread_t,
            THREAD_PRECEDENCE_POLICY as thread_policy_flavor_t,
            &mut prec as *mut _ as *mut i32,
            THREAD_PRECEDENCE_POLICY_COUNT,
        )
    };

    mach_to_result(ret, ())
}

/// Reads the nice value of a thread on macOS by reporting [set_thread_nice()]
/// as the precedence policy's importance.
#[cfg(target_os = "macos")]
fn get_thread_nice(pid: Pid) -> Result<i32, std::io::Error> {
    use crate::unix::macos::mach_to_result;
    use libc::{
        thread_policy_flavor_t, thread_policy_t, thread_precedence_policy_data_t, thread_t,
        THREAD_PRECEDENCE_POLICY, THREAD_PRECEDENCE_POLICY_COUNT,
    };

    let mut prec = thread_precedence_policy_data_t { importance: 0 };
    let mut count = THREAD_PRECEDENCE_POLICY_COUNT;
    let mut get_default = 0;
    let ret = unsafe {
        libc::thread_policy_get(
            pid.as_raw() as thread_t,
            THREAD_PRECEDENCE_POLICY as thread_policy_flavor_t,
            &mut prec as *mut _ as thread_policy_t,
            &mut count,
            &mut get_default,
        )
    };

    mach_to_result(ret, -prec.importance)
}

/// The precedence policy's importance reserved for the Idle policy on macOS
/// (see [set_attr()]). No valid nice value maps to it, because POSIX nice
/// values range from -20 to 19 and [set_thread_nice()] inverts their sign.
#[cfg(target_os = "macos")]
const DARWIN_IDLE_IMPORTANCE: i32 = -40;

/// The [set_attr()] function wraps the `sched_setattr()` system call and sets the scheduling policy and
/// associated attributes for the thread whose ID is specified in pid.
pub fn set_attr(pid: Pid, attr: Attributes) -> Result<(), std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        let mut attr = SchedAttr {
            size: core::mem::size_of::<SchedAttr>() as u32,
            sched_policy: attr.policy.into_raw(),
            sched_flags: attr.flags.bits() as u64,
            sched_nice: attr.nice,
            sched_priority: attr.priority,
            sched_runtime: attr.runtime_ns,
            sched_deadline: attr.deadline_ns,
            sched_period: attr.period_ns,
            sched_util_min: attr.sched_util_min,
            sched_util_max: attr.sched_util_max,
        };
        unsafe { sched_set_attr(pid.as_raw(), &mut attr, 0) }
            .or(Err(std::io::Error::last_os_error()))
            .and(Ok(()))
    }

    #[cfg(target_os = "macos")]
    {
        use libc::{
            thread_extended_policy_data_t, thread_precedence_policy_data_t, thread_t,
            thread_time_constraint_policy_data_t, THREAD_EXTENDED_POLICY,
            THREAD_EXTENDED_POLICY_COUNT, THREAD_PRECEDENCE_POLICY, THREAD_PRECEDENCE_POLICY_COUNT,
            THREAD_TIME_CONSTRAINT_POLICY, THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        };
        use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};

        use crate::unix::macos::mach_to_result;

        // SCHED_IDLE has no direct macOS equivalent; it maps to the Idle
        // importance value in [set_thread_nice()] and only applies without a
        // nice value (like the `EINVAL` of sched_setattr() on Linux).
        if attr.policy == Policy::Idle {
            if attr.nice != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "The Idle policy applies only without a nice value",
                ));
            }

            let mut prec = thread_precedence_policy_data_t {
                importance: DARWIN_IDLE_IMPORTANCE,
            };
            let ret = unsafe {
                libc::thread_policy_set(
                    pid.as_raw() as thread_t,
                    THREAD_PRECEDENCE_POLICY as u32,
                    &mut prec as *mut _ as *mut i32,
                    THREAD_PRECEDENCE_POLICY_COUNT,
                )
            };

            return mach_to_result(ret, ());
        }

        let mut ext = thread_extended_policy_data_t { timeshare: 0 };
        let mut tc = thread_time_constraint_policy_data_t {
            period: 0,
            computation: 0,
            constraint: 0,
            preemptible: 0,
        };

        let (flavor, count, data) = match attr.policy {
            Policy::Ext | Policy::Normal => {
                ext.timeshare = 1;
                (
                    THREAD_EXTENDED_POLICY,
                    THREAD_EXTENDED_POLICY_COUNT,
                    &mut ext as *const _ as *mut i32,
                )
            }
            Policy::Batch => {
                // SCHED_BATCH has no direct macOS equivalent; the closest
                // thread policy is the non-timeshare extended policy.
                ext.timeshare = 0;
                (
                    THREAD_EXTENDED_POLICY,
                    THREAD_EXTENDED_POLICY_COUNT,
                    &mut ext as *const _ as *mut i32,
                )
            }
            Policy::Fifo | Policy::RoundRobin | Policy::Deadline => {
                // THREAD_TIME_CONSTRAINT_POLICY expects absolute time units (mach ticks),
                // not nanoseconds. Use supplied params when present, otherwise conservative defaults.
                let mut timebase = mach_timebase_info_data_t { numer: 0, denom: 0 };
                let _ = unsafe { mach_timebase_info(&mut timebase) };

                let to_abs = |ns: u64| -> u32 {
                    let numer = u64::from(timebase.numer.max(1));
                    let denom = u64::from(timebase.denom.max(1));
                    let ticks = ns.saturating_mul(denom) / numer;
                    ticks.clamp(1, u64::from(u32::MAX)) as u32
                };

                let runtime_ns = if attr.runtime_ns > 0 {
                    attr.runtime_ns
                } else {
                    1_000_000
                };
                let constraint_ns = if attr.deadline_ns > 0 {
                    attr.deadline_ns.max(runtime_ns)
                } else {
                    5_000_000
                };
                let period_ns = if attr.period_ns > 0 {
                    attr.period_ns.max(constraint_ns)
                } else {
                    10_000_000
                };

                tc.period = to_abs(period_ns);
                tc.computation = to_abs(runtime_ns);
                tc.constraint = to_abs(constraint_ns);
                tc.preemptible = 1;

                (
                    THREAD_TIME_CONSTRAINT_POLICY,
                    THREAD_TIME_CONSTRAINT_POLICY_COUNT,
                    &mut tc as *const _ as *mut i32,
                )
            }
            Policy::Idle => unreachable!("handled above"),
        };

        let ret = unsafe {
            use libc::thread_policy_flavor_t;

            libc::thread_policy_set(
                pid.as_raw() as thread_t,
                flavor as thread_policy_flavor_t,
                data,
                count,
            )
        };

        mach_to_result(ret, ())?;

        set_thread_nice(pid, attr.nice)
    }
}

pub fn get_priority_max(pol: Policy) -> Result<isize, std::io::Error> {
    let ret = unsafe { libc::sched_get_priority_max(pol.into_raw() as c_int) };
    ret_into_result(ret).map(|val| val as isize)
}

pub fn get_priority_min(pol: Policy) -> Result<isize, std::io::Error> {
    let ret = unsafe { libc::sched_get_priority_min(pol.into_raw() as c_int) };
    ret_into_result(ret).map(|val| val as isize)
}

pub fn sched_yield() -> Result<(), std::io::Error> {
    let ret = unsafe { libc::sched_yield() };
    ret_to_result(ret, ())
}

#[cfg(target_os = "linux")]
pub fn set_affinity(pid: Pid, set: CpuSet) -> Result<(), std::io::Error> {
    let ret = unsafe {
        libc::sched_setaffinity(
            pid.as_raw(),
            CpuSet::size_of(),
            set.as_raw() as *const libc::cpu_set_t,
        )
    };
    ret_to_result(ret, ())
}
#[cfg(target_os = "macos")]
pub fn set_affinity(_pid: Pid, _set: CpuSet) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Affinity not supported",
    ))
}

#[cfg(target_os = "linux")]
pub fn get_affinity(pid: Pid) -> Result<CpuSet, std::io::Error> {
    let mut cpuset = CpuSet::empty();
    let ret = unsafe {
        libc::sched_getaffinity(
            pid.as_raw(),
            CpuSet::size_of(),
            cpuset.as_mut_raw() as *mut libc::cpu_set_t,
        )
    };

    ret_to_result(ret, cpuset)
}
#[cfg(target_os = "macos")]
pub fn get_affinity(_pid: Pid) -> Result<CpuSet, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Affinity not supported",
    ))
}

#[cfg(test)]
mod test {
    use crate::sched::Pid;

    use super::*;

    #[test]
    fn test_get_affinity() {
        let res = get_affinity(Pid::this());
        #[cfg(target_os = "linux")]
        assert!(res.is_ok());
        #[cfg(target_os = "macos")]
        assert!(res.is_err());
    }

    #[test]
    fn test_get_attr() {
        let res = get_attr(Pid::this());
        assert!(res.is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_set_attr() {
        let res = set_attr(
            Pid::this(),
            Attributes {
                policy: Policy::Normal,
                flags: SchedFlags::empty(),
                nice: 0,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            },
        );
        println!("Normal {res:?}");
        assert!(res.is_ok());

        let res = set_attr(
            Pid::this(),
            Attributes {
                policy: Policy::Batch,
                flags: SchedFlags::empty(),
                nice: 0,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            },
        );
        println!("Batch {res:?}");
        assert!(res.is_ok());

        let res = set_attr(
            Pid::this(),
            Attributes {
                policy: Policy::Idle,
                flags: SchedFlags::empty(),
                nice: 0,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            },
        );
        println!("Idle {res:?}");
        if let Err(err) = &res {
            assert!(
                matches!(
                    err.kind(),
                    std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::Unsupported
                ),
                "unexpected Idle error: {err:?}"
            );
        }

        let res = set_attr(
            Pid::this(),
            Attributes {
                policy: Policy::Fifo,
                flags: SchedFlags::empty(),
                nice: 0,
                priority: 0,
                runtime_ns: 0,
                deadline_ns: 0,
                period_ns: 0,
                sched_util_min: 0,
                sched_util_max: 0,
            },
        );
        println!("Fifo {res:?}");
        if let Err(err) = &res {
            assert!(
                matches!(
                    err.kind(),
                    std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::Unsupported
                ),
                "unexpected Fifo error: {err:?}"
            );
        }

        let res = get_attr(Pid::this());
        assert!(res.is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn set_attr() {
        use super::*;
        use std::mem;
        let mut attr = SchedAttr {
            size: mem::size_of::<SchedAttr>() as u32,
            sched_policy: SCHED_IDLE,
            sched_flags: 0,
            sched_nice: 0,
            sched_priority: 0,
            sched_runtime: 0,
            sched_deadline: 0,
            sched_period: 0,
            sched_util_min: 0,
            sched_util_max: 0,
        };
        let ret = unsafe { sched_set_attr(0, &mut attr, 0) };
        assert_eq!(ret, Ok(0));

        let mut attr2 = unsafe { mem::zeroed::<SchedAttr>() };
        let ret = unsafe { sched_get_attr(0, &mut attr2, mem::size_of::<SchedAttr>() as u32, 0) };
        assert_eq!(ret, Ok(0));
        assert_eq!(attr2.sched_policy, { SCHED_IDLE });
    }
}
