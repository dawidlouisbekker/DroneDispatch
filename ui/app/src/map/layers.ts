// GeoJSON for the map, shared by the web and native map components.
import type { Feature, FeatureCollection, LineString, Point, Polygon, Position } from 'geojson';

import type { DroneSample, FleetSnapshot, LatLon } from '@/api/types';

export type DroneProperties = {
  drone_id: string;
  state: string;
  battery_pct: number;
  heading_deg: number;
  /** Flying one of the signed-in customer's orders. */
  mine: boolean;
};

const position = (p: LatLon): Position => [p.lon, p.lat];

const collection = <G extends Point | LineString | Polygon, P>(
  features: Feature<G, P>[],
): FeatureCollection<G, P> => ({ type: 'FeatureCollection', features });

export function zonesGeoJson(snapshot: FleetSnapshot): FeatureCollection<Polygon, { zone_id: string; name: string }> {
  return collection(
    snapshot.zones.map((zone) => ({
      type: 'Feature',
      properties: { zone_id: zone.zone_id, name: zone.name },
      geometry: { type: 'Polygon', coordinates: [zone.boundary] },
    })),
  );
}

export function noFlyGeoJson(snapshot: FleetSnapshot): FeatureCollection<Polygon, { name: string }> {
  return collection(
    snapshot.no_fly_zones.map((area) => ({
      type: 'Feature',
      properties: { name: area.name },
      geometry: { type: 'Polygon', coordinates: [area.boundary] },
    })),
  );
}

export function hubsGeoJson(snapshot: FleetSnapshot): FeatureCollection<Point, { hub_id: string }> {
  return collection(
    snapshot.hubs.map((hub) => ({
      type: 'Feature',
      properties: { hub_id: hub.hub_id },
      geometry: { type: 'Point', coordinates: position(hub.position) },
    })),
  );
}

export function dronesGeoJson(
  drones: Iterable<DroneSample>,
  myOrderIds: ReadonlySet<string>,
): FeatureCollection<Point, DroneProperties> {
  return collection(
    Array.from(drones, (drone) => ({
      type: 'Feature' as const,
      id: drone.drone_id,
      properties: {
        drone_id: drone.drone_id,
        state: drone.state,
        battery_pct: drone.battery_pct,
        heading_deg: drone.heading_deg,
        mine: drone.order_id != null && myOrderIds.has(drone.order_id),
      },
      geometry: { type: 'Point' as const, coordinates: position(drone.position) },
    })),
  );
}

/** A path through `points`, e.g. hub → pickup → drop-off. */
export function routeGeoJson(points: LatLon[]): FeatureCollection<LineString, Record<string, never>> {
  if (points.length < 2) {
    return collection([]);
  }
  return collection([
    { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: points.map(position) } },
  ]);
}

/** Markers for the pickup and drop-off of the order being tracked. */
export function stopsGeoJson(stops: { kind: 'pickup' | 'dropoff'; position: LatLon }[]): FeatureCollection<
  Point,
  { kind: 'pickup' | 'dropoff' }
> {
  return collection(
    stops.map((stop) => ({
      type: 'Feature',
      properties: { kind: stop.kind },
      geometry: { type: 'Point', coordinates: position(stop.position) },
    })),
  );
}
