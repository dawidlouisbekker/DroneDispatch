// Domain types generated from api/openapi/*.yaml (`pnpm gen:api`). Import from here, not from ./generated.
import type { components as CatalogComponents } from './generated/catalog';
import type { components as MerchantComponents } from './generated/merchant';
import type { components as UserComponents } from './generated/user';

type UserSchemas = UserComponents['schemas'];
type MerchantSchemas = MerchantComponents['schemas'];
type CatalogSchemas = CatalogComponents['schemas'];

export type Money = UserSchemas['Money'];
export type LatLon = UserSchemas['LatLon'];
export type Problem = UserSchemas['Problem'];

// Customer API (user-service)
export type Me = UserSchemas['Me'];
export type Order = UserSchemas['Order'];
export type OrderState = UserSchemas['OrderState'];
export type OrderChannel = UserSchemas['OrderChannel'];
export type OrderPage = UserSchemas['OrderPage'];
export type PickupLocation = UserSchemas['PickupLocation'];
export type NewPickupLocation = UserSchemas['NewPickupLocation'];
export type Station = UserSchemas['Station'];
export type StationInput = UserSchemas['StationInput'];
export type AccessNetwork = UserSchemas['AccessNetwork'];
export type PlaceSuggestion = UserSchemas['PlaceSuggestion'];
export type SpendPolicy = UserSchemas['SpendPolicy'];
export type RedirectLink = UserSchemas['RedirectLink'];
export type FleetSnapshot = UserSchemas['FleetSnapshot'];
export type Zone = UserSchemas['Zone'];
export type Hub = UserSchemas['Hub'];
export type NoFlyZone = UserSchemas['NoFlyZone'];
export type DroneSample = UserSchemas['DroneSample'];
export type DroneState = UserSchemas['DroneState'];
export type MissionEventKind = UserSchemas['MissionEventKind'];
export type LiveServerMessage = UserSchemas['LiveServerMessage'];

// Catalog API (catalog-read-service, read-only)
export type Catalog = CatalogSchemas['Catalog'];
export type CatalogSection = CatalogSchemas['CatalogSection'];
export type CatalogItem = CatalogSchemas['CatalogItem'];
export type CatalogMenu = CatalogSchemas['Menu'];

// Merchant API (merchant-service)
export type Business = MerchantSchemas['Business'];
export type BoardOrder = MerchantSchemas['BoardOrder'];
export type BoardOrderStatus = MerchantSchemas['BoardOrderStatus'];
export type Menu = MerchantSchemas['Menu'];
export type MenuSection = MerchantSchemas['MenuSection'];
export type MenuItem = MerchantSchemas['MenuItem'];
export type MenuItemInput = MerchantSchemas['MenuItemInput'];
export type MenuItemPatch = MerchantSchemas['MenuItemPatch'];
export type BusinessStation = MerchantSchemas['Station'];
export type StationRegistration = MerchantSchemas['StationRegistration'];
export type PayoutStatus = MerchantSchemas['PayoutStatus'];
export type BoardServerMessage = MerchantSchemas['BoardServerMessage'];
