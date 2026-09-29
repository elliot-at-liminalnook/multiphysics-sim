// Alignment pose of the leg mirror: the knee aligns at mid-travel (the printed
// leg cannot reach its CAD home), other motors at CAD home, and a saved
// alignment keeps the joint angle it was captured at.
import assert from 'node:assert/strict';
import {LegMirror} from '../viewer/calibration-mirror.mjs';

const coordinates = [
  {joint: '+X | Foot servo output', home: 0, lower: -2.5743606466916362, upper: 0.07853981633974483},
  {joint: '+X | Worm servo output', home: 0, lower: null, upper: null},
];
const mirror = {coordinates, settings: {leg: '+X', bindings: {
  1: {joint: 'Foot servo output', polarity: -1, align: 'mid'},
  2: {joint: 'Worm servo output', polarity: 1, align: 'mid'},
  3: {joint: 'Foot servo output', polarity: 1, align: 'home'},
}}};
mirror.joint = LegMirror.prototype.joint;
const angle = id => LegMirror.prototype.alignmentAngle.call(mirror, id);
assert.ok(Math.abs(angle(1) - (-1.2479104151759457)) < 1e-12, 'knee aligns halfway between its CAD limits');
assert.equal(angle(2), 0, 'a joint without CAD limits falls back to CAD home');
assert.equal(angle(3), 0, 'CAD home when chosen');
assert.equal(LegMirror.prototype.alignmentAngle.call({...mirror, coordinates: null, joint: mirror.joint}, 1), undefined, 'no angle before the model loads');
assert.equal(LegMirror.savedAngle({reference: 2900, reference_joint_rad: -1.25}, coordinates[0]), -1.25);
assert.equal(LegMirror.savedAngle({reference: 2722}, coordinates[0]), 0, 'older saves were made at CAD home');
console.log('calibration mirror alignment: ok');
