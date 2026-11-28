//! Biome weather queries for a live level: `ServerLevel.tickPrecipitation`'s
//! inputs (`Biome.shouldFreeze`, `shouldSnow`, `getPrecipitationAt`) and
//! `Level.isRainingAt`, from the biome temperatures of the worldgen bundle.
use super::{Behaviours, LevelAccess, Precipitation, PrecipitationKind};
use crate::{
    fluid::FluidKind,
    worldgen::biome_temperature::{BiomeTemperatureSettings, Precipitation as BiomePrecipitation},
};
use horizon_core::{geometry::Direction, position::BlockPos};

fn sea_level<L: LevelAccess + ?Sized>(level: &L) -> i32 {
    level.sea_level()
}

impl Behaviours {
    fn biome_settings<L: LevelAccess + ?Sized>(
        &self,
        level: &L,
        pos: BlockPos,
    ) -> Option<BiomeTemperatureSettings> {
        let features = self.live_features()?;
        let biome = features.biome_id(level.biome_name(pos)?)?;
        features.biome_temperature(biome)
    }
    /// `Biome.getPrecipitationAt(pos, seaLevel)`.
    fn precipitation_kind(
        settings: BiomeTemperatureSettings,
        pos: BlockPos,
        sea: i32,
    ) -> BiomePrecipitation {
        if !settings.has_precipitation {
            BiomePrecipitation::None
        } else if settings.height_adjusted(pos, sea) < 0.15 {
            BiomePrecipitation::Snow
        } else {
            BiomePrecipitation::Rain
        }
    }
    /// `getHeightmapPos(MOTION_BLOCKING, pos)`, scanning down from the
    /// world surface (air neither blocks motion nor holds a fluid).
    pub fn motion_blocking_top<L: LevelAccess + ?Sized>(
        &self,
        level: &L,
        pos: BlockPos,
    ) -> BlockPos {
        let surface_y = level.world_surface(pos.x, pos.z).min(level.max_y());
        self.motion_blocking_top_from_surface(level, pos, surface_y)
    }

    fn motion_blocking_top_from_surface<L: LevelAccess + ?Sized>(
        &self,
        level: &L,
        pos: BlockPos,
        surface_y: i32,
    ) -> BlockPos {
        let blocks = self.tag("minecraft:blocks_motion_in_heightmap");
        for y in (level.min_y()..surface_y).rev() {
            let state = level.block_state(BlockPos::new(pos.x, y, pos.z));
            let fluid = self.fluid(state).kind() != FluidKind::Empty;
            if fluid || blocks.is_some_and(|t| t.contains(self.block(state))) {
                return BlockPos::new(pos.x, y + 1, pos.z);
            }
        }
        BlockPos::new(pos.x, level.min_y(), pos.z)
    }
    /// Answers for one `ServerLevel.tickPrecipitation` at the column of `pos`;
    /// None without live features or a biome.
    pub fn precipitation_info<L: LevelAccess + ?Sized>(
        &self,
        level: &L,
        pos: BlockPos,
    ) -> Option<Precipitation> {
        let surface_y = level.world_surface(pos.x, pos.z).min(level.max_y());
        let top = self.motion_blocking_top_from_surface(level, pos, surface_y);
        let below = BlockPos::new(top.x, top.y - 1, top.z);
        let settings = self.biome_settings(level, top)?;
        let force_snow = level.force_snow();
        let sea = sea_level(level);
        let kind_at = |pos| {
            if force_snow {
                BiomePrecipitation::Snow
            } else {
                Self::precipitation_kind(settings, pos, sea)
            }
        };
        let inside = |p: BlockPos| p.y >= level.min_y() && p.y < level.max_y();
        // Biome.shouldFreeze(level, below, true)
        let freezes_below = (force_snow || settings.height_adjusted(below, sea) < 0.15)
            && inside(below)
            && level.block_light(below) < 10
            && {
                let state = level.block_state(below);
                self.block(state) == self.named.water && self.fluid(state).is_source()
            }
            && [
                Direction::West,
                Direction::East,
                Direction::North,
                Direction::South,
            ]
            .into_iter()
            .any(|d| {
                let n = super::update::rel(below, d);
                !self.is_water_fluid(self.fluid(level.block_state(n)).kind())
            });
        // Biome.shouldSnow(level, top)
        let snows_at_top = kind_at(top) == BiomePrecipitation::Snow
            && inside(top)
            && level.block_light(top) < 10
            && {
                let state = level.block_state(top);
                (self.is_air(state) || self.block(state) == self.named.snow)
                    && self.snow_survives(level, top)
            };
        let kind = match kind_at(below) {
            BiomePrecipitation::None => PrecipitationKind::None,
            BiomePrecipitation::Rain => PrecipitationKind::Rain,
            BiomePrecipitation::Snow => PrecipitationKind::Snow,
        };
        Some(Precipitation {
            top,
            surface_y,
            freezes_below,
            snows_at_top,
            kind,
        })
    }

    pub(super) fn warm_enough_to_melt<L: LevelAccess + ?Sized>(
        &self,
        level: &L,
        pos: BlockPos,
    ) -> bool {
        // Biome.warmEnoughToRain is the inverse of coldEnoughToSnow, even in dry biomes.
        self.biome_settings(level, pos)
            .is_some_and(|settings| settings.height_adjusted(pos, sea_level(level)) >= 0.15)
    }

    /// `SnowLayerBlock.canSurvive` for a default snow layer at `pos`.
    fn snow_survives<L: LevelAccess + ?Sized>(&self, level: &L, pos: BlockPos) -> bool {
        let below = level.block_state(BlockPos::new(pos.x, pos.y - 1, pos.z));
        let block = self.block(below);
        if self.tags.cannot_support_snow_layer.contains(block) {
            return false;
        }
        if self.tags.support_override_snow_layer.contains(block) {
            return true;
        }
        self.collision_face_full(below, Direction::Up)
            || (block == self.named.snow && self.table.int(below, self.props.layers) == 8)
    }
    /// `Level.isRainingAt(pos)`.
    pub fn is_raining_at<L: LevelAccess + ?Sized>(&self, level: &L, pos: BlockPos) -> bool {
        if !level.is_raining() || level.sky_light(pos) < 15 {
            return false;
        }
        if self.motion_blocking_top(level, pos).y > pos.y {
            return false;
        }
        if level.force_snow() {
            return false;
        }
        self.biome_settings(level, pos).is_some_and(|s| {
            Self::precipitation_kind(s, pos, sea_level(level)) == BiomePrecipitation::Rain
        })
    }
}
