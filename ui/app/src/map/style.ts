import type { LatLon } from '@/api/types';
import { config } from '@/config';

export const SEATTLE: LatLon = { lat: 47.6062, lon: -122.3321 };

/**
 * Amazon Location Maps "Standard" style. Without a Maps API key the map uses
 * a plain background, so zones, routes and drones still render.
 */
export function mapStyle(): string | object {
  if (!config.mapsApiKey) {
    return BLANK_STYLE;
  }
  const key = encodeURIComponent(config.mapsApiKey);
  return `https://maps.geo.${config.awsRegion}.amazonaws.com/v2/styles/Standard/descriptor?key=${key}`;
}

const BLANK_STYLE = {
  version: 8,
  sources: {},
  layers: [{ id: 'background', type: 'background', paint: { 'background-color': '#e9eef2' } }],
};
