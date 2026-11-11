import { expect, test } from "bun:test";
import { inSentence, type Region, Regions } from "../src/regions";
import { ago, distanceKm, type PathPoint, roundKm, tellStory } from "../src/story";

const country = (name: string): Region => ({ name, kind: "country" });

test("regions read naturally after 'over'", () => {
  expect(inSentence(country("Sweden"))).toBe("Sweden");
  expect(inSentence(country("United States of America"))).toBe("the United States");
  expect(inSentence(country("Netherlands"))).toBe("the Netherlands");
  expect(inSentence({ name: "North Sea", kind: "sea" })).toBe("the North Sea");
  expect(inSentence({ name: "Atlantic Ocean", kind: "ocean" })).toBe("the Atlantic Ocean");
  expect(inSentence({ name: "Bay of Biscay", kind: "bay" })).toBe("the Bay of Biscay");
  expect(inSentence({ name: "Hudson Bay", kind: "bay" })).toBe("Hudson Bay");
  expect(inSentence(country("Falkland Is."))).toBe("the Falkland Islands");
});

test("the region grid names points and reaches for a neighbour", () => {
  // 8 × 4 cells of 45°: the north-west corner is region 1, the rest unnamed
  // except one cell of region 2 just east of the Greenwich meridian.
  const grid = new Uint16Array(32);
  grid[0] = 1;
  grid[1 * 8 + 4] = 2;
  const regions = new Regions(
    { width: 8, height: 4, names: ["", "Northland", "Midsea"], kinds: ["", "country", "sea"] },
    grid,
  );
  expect(regions.at(-170, 80)).toEqual(country("Northland"));
  expect(regions.at(190, 80)).toEqual(country("Northland"));
  expect(regions.at(10, 20)).toEqual({ name: "Midsea", kind: "sea" });
  expect(regions.at(60, 20)?.name).toBe("Midsea");
});

test("small helpers", () => {
  expect(ago(0)).toBe("now");
  expect(ago(5)).toBe("5 hours ago");
  expect(ago(24)).toBe("yesterday");
  expect(ago(240)).toBe("10 days ago");
  expect(roundKm(87)).toBe("90");
  expect(roundKm(11432)).toBe("11,400");
  const at = (lon: number): PathPoint => ({ lon, lat: 0, hpa: 0, heightM: 0 });
  expect(distanceKm(at(0), at(90))).toBeCloseTo(10007.9, 0);
  expect(distanceKm(at(359), at(1))).toBeCloseTo(222.4, 0);
});

test("a path becomes a story", () => {
  // 48 hours due west along 60°N at half a degree per hour, climbing.
  const path: PathPoint[] = Array.from({ length: 49 }, (_, hour) => ({
    lon: (20 - hour * 0.5 + 360) % 360,
    lat: 60,
    hpa: 950 - hour * 5,
    heightM: 500 + hour * 40,
  }));
  const regionAt = ({ lon }: PathPoint): Region | undefined => {
    const east = lon > 180 ? lon - 360 : lon;
    if (east > 11) return country("Sweden");
    if (east > 10.4) return country("Sliver");
    if (east > 5) return country("Norway");
    return { name: "North Sea", kind: "sea" };
  };
  const story = tellStory("Stockholm", path, regionAt);
  expect(story.headline).toBe("2 days ago, the air above Stockholm was over the North Sea.");
  expect(story.hours).toBe(48);
  expect(story.pathKm).toBeCloseTo(48 * distanceKm(path[0] as PathPoint, path[1] as PathPoint), 3);
  expect(story.originKm).toBeLessThan(story.pathKm);
  expect(story.peakHoursAgo).toBe(48);
  expect(story.peakM).toBe(500 + 48 * 40);
  expect(story.topSpeedKmh).toBeCloseTo(27.8, 0);
  expect(story.cumulativeKm.length).toBe(49);
  expect(story.legs.map((leg) => leg.region?.name)).toEqual(["Sweden", "Norway", "North Sea"]);
  expect(story.legs[0]?.untilHoursAgo).toBe(0);
  expect(story.legs.at(-1)?.fromHoursAgo).toBe(48);
  for (const [index, leg] of story.legs.entries()) {
    const next = story.legs[index + 1];
    if (next) expect(leg.fromHoursAgo + 1).toBe(next.untilHoursAgo);
  }
});

test("without place names the story gives distances and claims nothing else", () => {
  const path: PathPoint[] = Array.from({ length: 25 }, (_, hour) => ({
    lon: 20 - hour * 0.5,
    lat: 60,
    hpa: 950,
    heightM: 500,
  }));
  const story = tellStory("Stockholm", path, () => undefined);
  expect(story.headline).toBe("Yesterday, the air above Stockholm was 670 km away.");
  expect(story.origin).toBeUndefined();
  expect(story.legs.length).toBe(1);
});
