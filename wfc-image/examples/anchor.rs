extern crate grid_2d;
extern crate hashbrown;
extern crate image;
extern crate rand;
extern crate rand_xorshift;
extern crate simon;
extern crate wfc;
extern crate wfc_image;

use grid_2d::coord_system::XThenYIter;
use hashbrown::*;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use simon::*;
use std::num::NonZeroU32;
use wfc::retry::*;
use wfc::*;
use wfc_image::*;

struct Args {
    output_size: Size,
    pattern_size: u32,
    seed: u64,
    input_image: image::DynamicImage,
    output_path: String,
    orientations: &'static [orientation::Orientation],
    retries: usize,
    allow_corner: bool,
}

impl Args {
    fn arg() -> ArgExt<impl Arg<Item = Self>> {
        args_map! {
            let {
                width = opt_default("x", "width", "output width", "INT", 48);
                height = opt_default("y", "height", "output height", "INT", 48);
                pattern_size = opt_default("p", "pattern-size", "pattern size", "INT", 3);
                seed = opt("s", "seed", "rng seed", "INT")
                    .map(|seed| seed.unwrap_or_else(|| rand::thread_rng().gen()));
                input_path = opt_required::<String>("i", "input-path", "input path", "PATH");
                output_path = opt_required("o", "output-path", "output path", "PATH");
                all_orientations = flag("a", "all-orientations", "include all orientations");
                retries = opt_default("r", "retries", "number of retries", "INT", 10);
                allow_corner = flag("c", "allow-corner", "allow bottom right corner");
            } in {
                Self {
                    output_size: Size::new(width, height),
                    pattern_size,
                    seed,
                    input_image: image::open(input_path).unwrap(),
                    output_path,
                    orientations: if all_orientations {
                        &orientation::ALL
                    } else {
                        &[Orientation::Original]
                    },
                    retries,
                    allow_corner,
                }
            }
        }
    }
}

struct Forbid {
    bottom_right_id: PatternId,
    ids_to_forbid_bottom_right: HashSet<PatternId>,
    ids_to_forbid_centre: HashSet<PatternId>,
    offset: i32,
}

impl ForbidPattern for Forbid {
    fn forbid<W: Wrap, R: Rng>(&mut self, fi: &mut ForbidInterface<W>, rng: &mut R) {
        let output_size = fi.wave_size();
        let bottom_right_coord = Coord::new(
            output_size.width() as i32 - self.offset,
            output_size.height() as i32 - self.offset,
        );
        fi.forbid_all_patterns_except(bottom_right_coord, self.bottom_right_id, rng)
            .unwrap();
        for coord in XThenYIter::new(output_size) {
            let delta = coord - bottom_right_coord;
            if delta.magnitude2() > 2 {
                for &id in self.ids_to_forbid_bottom_right.iter() {
                    fi.forbid_pattern(coord, id, rng).unwrap();
                }
            }
            let pad = 8;
            if coord.x > pad
                && coord.y > pad
                && coord.x < output_size.width() as i32 - pad
                && coord.y < output_size.height() as i32 - pad
            {
                for &id in self.ids_to_forbid_centre.iter() {
                    fi.forbid_pattern(coord, id, rng).unwrap();
                }
            }
        }
    }
}

fn app(args: Args) -> Result<(), ()> {
    let mut rng = XorShiftRng::seed_from_u64(args.seed);
    let mut image_patterns = ImagePatterns::new(
        &args.input_image,
        NonZeroU32::new(args.pattern_size).expect("pattern size may not be zero"),
        args.orientations,
    );
    let input_size = image_patterns.grid().size();
    let bottom_right_offset = args.pattern_size - (args.pattern_size / 2);
    let id_grid = image_patterns.id_grid();
    let bottom_right_coord = Coord::new(
        input_size.width() as i32 - bottom_right_offset as i32,
        input_size.height() as i32 - bottom_right_offset as i32,
    );
    let bottom_right_ids = id_grid
        .get_checked(bottom_right_coord)
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let top_left_ids = [
        Coord::new(0, 0),
        Coord::new(1, 1),
        Coord::new(0, 8),
        Coord::new(0, 32),
        Coord::new(26, 1),
        Coord::new(59, 18),
        Coord::new(14, 59),
        Coord::new(7, 1),
    ]
    .iter()
    .flat_map(|&coord| id_grid.get_checked(coord).iter().cloned())
    .collect::<HashSet<_>>();
    for &empty_id in id_grid.get_checked(Coord::new(8, 8)).iter() {
        image_patterns.pattern_mut(empty_id).clear_count();
    }
    let bottom_right_id = *id_grid
        .get_checked(bottom_right_coord)
        .get(Orientation::Original)
        .unwrap();
    if !args.allow_corner {
        bottom_right_ids.iter().for_each(|&pattern_id| {
            image_patterns.pattern_mut(pattern_id).clear_count();
        });
    }
    let global_stats = image_patterns.global_stats();
    let mut wave = Wave::new(args.output_size);
    let mut context = Context::new();
    let result = {
        let forbid = Forbid {
            bottom_right_id,
            ids_to_forbid_bottom_right: bottom_right_ids,
            ids_to_forbid_centre: top_left_ids,
            offset: bottom_right_offset as i32,
        };
        let mut run = RunBorrow::new_forbid(
            &mut context,
            &mut wave,
            &global_stats,
            forbid,
            &mut rng,
        );
        run.collapse_retrying(NumTimes(args.retries), &mut rng)
    };
    match result {
        Err(_) => {
            eprintln!("Too many contradictions!");
            Err(())
        }
        Ok(()) => {
            image_patterns
                .image_from_wave(&wave)
                .save(args.output_path)
                .unwrap();
            Ok(())
        }
    }
}

fn main() {
    let args = Args::arg().with_help_default().parse_env_default_or_exit();
    ::std::process::exit(match app(args) {
        Ok(()) => 0,
        Err(()) => 1,
    })
}
